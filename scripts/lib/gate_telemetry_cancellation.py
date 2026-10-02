#!/usr/bin/env python3
"""Bounded controls for actual telemetry CLI and nested ownership delegation."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import tempfile
import time

import generated_authority as ga
import gate_telemetry as telemetry
from generated_authority_cancellation import group_alive, wait_for


def worker(root: Path, scope: Path, case: str) -> int:
    ready = scope / 'ready.json'
    output = scope / 'report.json'
    payload = (
        "import json,os,signal,time; from pathlib import Path; "
        "[(signal.signal(s,signal.SIG_IGN)) for s in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP)]; "
        f"Path({str(ready)!r}).write_text(json.dumps({{'pid':os.getpid(),'pgid':os.getpgrp()}})); "
        "time.sleep(30)"
    )
    command = [sys.executable, '-c', payload]
    if case == 'leader-exit':
        command = [sys.executable, '-c',
                   f"import subprocess,time; from pathlib import Path; subprocess.Popen({command!r}); "
                   f"p=Path({str(ready)!r});\nwhile not p.exists(): time.sleep(.01)"]
    if case in ('nested', 'timeout', 'checks'):
        # Two wrappers must borrow the same owning group, including standalone
        # wrappers inside an aggregate's command.
        for _ in range(2):
            command = [sys.executable, str(root / 'scripts/lib/gate_telemetry.py'),
                       '--root', str(root), '--entrypoint', 'scripts/check_doc_hygiene.sh',
                       '--emit', 'none', '--', *command]
        owner = ga.AggregateResourceOwner(scope, scope, {'maxTimeoutSeconds':20,'maxDiskMiB':64}, sampling_root=root)
        try:
            def selected(_argv):
                if case == 'checks':
                    import shlex
                    scripts=scope/'scripts';scripts.mkdir()
                    (scripts/'wrapper.sh').write_text('exec ' + shlex.join(command) + '\n')
                    ga.run_checks(scope,[{'checks':['scripts/wrapper.sh'],'timeoutSeconds':15}],owner)
                else:
                    ga.run_bounded(command,cwd=root,timeout=1 if case=='timeout' else 15,owner=owner)
                return 0
            ga.run_main = selected
            try:
                return ga.main([])
            except subprocess.TimeoutExpired:
                return 1
        finally:
            owner.close(validate=False)
            shutil.rmtree(owner.temporary_root)
    if case in ('sampler-failure','registration'):
        if case == 'sampler-failure':
            def failed_sample(self):
                wait_for(ready.exists,'sampler child readiness')
                raise OSError('injected sampler failure')
            telemetry.Sampler.sample = failed_sample
        else:
            popen = subprocess.Popen
            def interrupted_spawn(*args,**kwargs):
                proc=popen(*args,**kwargs)
                wait_for(ready.exists,'telemetry spawn readiness')
                os.kill(os.getpid(),signal.SIGTERM)
                return proc
            telemetry.subprocess.Popen = interrupted_spawn
        try:
            return telemetry.run(root,'scripts/check_doc_hygiene.sh',command,output,'none')
        except telemetry.TelemetryError:
            return 2
    return telemetry.main(['--root',str(root),'--entrypoint','scripts/check_doc_hygiene.sh','--out',str(output),'--emit','none','--',*command])


def cancellation_self_test(root: Path) -> int:
    if os.name == 'nt':
        print('gate-telemetry-cancellation: POSIX process ownership unsupported on Windows')
        return 0
    cases = [(case, sig) for case in ('standalone','nested','checks') for sig in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP)]
    cases += [(case,None) for case in ('timeout','leader-exit','sampler-failure','registration')]
    observations=[]
    with tempfile.TemporaryDirectory(prefix='gate-telemetry-cancellation-controls-') as temporary:
        for index,(case,sig) in enumerate(cases):
            scope=Path(temporary)/str(index);scope.mkdir()
            events=scope/'events';events.mkdir()
            ready=scope/'ready.json'
            env=dict(os.environ,TMPDIR=str(events),TMP=str(events),TEMP=str(events))
            for key in (telemetry.AGGREGATE_OWNER_FD_ENV,telemetry.PROCESS_GROUP_OWNER_FD_ENV,telemetry.RETIRED_BUDGET_BYPASS_ENV,telemetry.EVENT_ROOT_ENV):env.pop(key,None)
            with (scope/'worker.log').open('wb') as log:
                process=subprocess.Popen([sys.executable,str(Path(__file__).resolve()),'--worker',case,'--root',str(root),'--scope',str(scope)],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
                child_group=None
                try:
                    wait_for(lambda:ready.exists() or process.poll() is not None,'actual child readiness')
                    ga.require(ready.exists(),(scope/'worker.log').read_text())
                    child_group=json.loads(ready.read_text())['pgid']
                    if sig is not None:process.send_signal(sig)
                    result=process.wait(timeout=8)
                    expected=128+sig if sig is not None else {'timeout':1,'leader-exit':0,'sampler-failure':2,'registration':143}[case]
                    ga.require(result==expected,f'{case}: expected {expected}, got {result}: {(scope/"worker.log").read_text()}')
                    wait_for(lambda:not group_alive(child_group),'nested child group cleanup')
                    ga.require(not list(events.glob('genesis-gate-*')),'standalone event channel survived cleanup')
                    # Aggregate-owned private events are also deleted by telemetry;
                    # the aggregate owner root remains until this fixture releases it.
                    ga.require(not list(scope.rglob('genesis-gate-events.*')),'nested event channel survived cleanup')
                    if case in ('standalone','registration'):
                        record=json.loads((scope/'report.json').read_text())
                        ga.require(record['result']=={'exitCode':expected,'status':'signaled'},'cancellation observation lost signal status')
                    if case=='leader-exit':
                        ga.require(json.loads((scope/'report.json').read_text())['result']=={'exitCode':0,'status':'passed'},'leader success classification changed')
                    observations.append({'case':case,'signal':sig,'returncode':result})
                finally:
                    ga.kill_and_reap(process)
                    if ready.exists():child_group=json.loads(ready.read_text())['pgid']
                    if child_group is not None and group_alive(child_group):os.killpg(child_group,signal.SIGKILL)
    # Descriptor validation never accepts an environment-only or read-only claim.
    saved=os.environ.get(telemetry.PROCESS_GROUP_OWNER_FD_ENV)
    try:
        for raw in ('2','-1','no-fd','999999','2147483648','9'*10000):
            os.environ[telemetry.PROCESS_GROUP_OWNER_FD_ENV]=raw
            try:telemetry.process_group_owner_fd()
            except telemetry.TelemetryError:pass
            else:raise ga.AuthorityError('invalid process-group descriptor accepted')
        with tempfile.NamedTemporaryFile() as file:
            fd=os.open(file.name,os.O_RDONLY);os.set_inheritable(fd,True)
            try:
                os.environ[telemetry.PROCESS_GROUP_OWNER_FD_ENV]=str(fd)
                try:telemetry.process_group_owner_fd()
                except telemetry.TelemetryError:pass
                else:raise ga.AuthorityError('read-only regular group descriptor accepted')
            finally:os.close(fd)
    finally:
        if saved is None:os.environ.pop(telemetry.PROCESS_GROUP_OWNER_FD_ENV,None)
        else:os.environ[telemetry.PROCESS_GROUP_OWNER_FD_ENV]=saved
    ga.require(len(observations)==13,'telemetry ownership control inventory drift')
    print('gate-telemetry-cancellation: '+json.dumps({'controls':observations,'descriptor_rejections':7},sort_keys=True))
    return 20


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--root',type=Path,default=ga.ROOT);parser.add_argument('--worker');parser.add_argument('--scope',type=Path);args=parser.parse_args()
    if args.worker:raise SystemExit(worker(args.root,args.scope,args.worker))
    cancellation_self_test(args.root)
