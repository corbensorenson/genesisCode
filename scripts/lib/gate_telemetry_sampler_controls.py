#!/usr/bin/env python3
"""Actual bounded-process controls for the telemetry inventory owner."""
from __future__ import annotations

import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time
from unittest.mock import patch

import gate_telemetry as telemetry


def sampler_self_test() -> int:
    if os.name == 'nt':
        raise telemetry.TelemetryError('POSIX inventory owner controls unsupported on Windows')
    controls = 0
    popen = subprocess.Popen
    with tempfile.TemporaryDirectory(prefix='telemetry-sampler-controls-') as temporary:
        scope = Path(temporary)
        for case in ('rss', 'exact', 'oversize', 'nonzero', 'nonascii', 'timeout', 'cancel', 'descendant'):
            sampler = telemetry.Sampler(os.getpid(), 20)
            sampler.platform = 'darwin'
            ready = scope / case
            inventory = f'{sampler.pid} 0 7\n900001 {sampler.pid} 11\n'
            code = (
                'import os,signal,time; from pathlib import Path; '
                '[signal.signal(s,signal.SIG_IGN) for s in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP)]; '
                f'Path({str(ready)!r}).write_text(str(os.getpid())); '
            )
            if case == 'rss':
                code += f'os.write(1,{inventory.encode()!r})'
            elif case in ('exact', 'oversize'):
                code += "[os.write(1,b' ' * 65536) for _ in range(16)]; "
                if case == 'oversize':
                    code += "os.write(1,b' '); time.sleep(30)"
            elif case == 'nonzero':
                code += 'raise SystemExit(7)'
            elif case == 'nonascii':
                code += "os.write(1,b'\\xff')"
            elif case == 'descendant':
                child_ready = scope / 'descendant-child'
                child = ('import os,signal,time;from pathlib import Path; '
                         'signal.signal(signal.SIGTERM,signal.SIG_IGN); '
                         f'Path({str(child_ready)!r}).write_text(str(os.getpgrp())); time.sleep(30)')
                code += ('import subprocess; '
                         f'subprocess.Popen([{sys.executable!r},"-c",{child!r}],stdout=subprocess.DEVNULL); '
                         f'p=Path({str(child_ready)!r});\nwhile not p.exists(): time.sleep(.01)')
            else:
                code += 'time.sleep(30)'
            helpers = []
            def spawn(argv, **kwargs):
                if argv != [sys.executable, '-I', '-B', str(Path(telemetry.darwin_inventory.__file__).resolve()), str(sampler.pid)]:
                    raise telemetry.TelemetryError('inventory argv drift')
                proc = popen([sys.executable, '-c', code], **kwargs)
                helpers.append(proc)
                return proc
            thread = None
            try:
                with patch.object(telemetry.subprocess, 'Popen', spawn):
                    if case == 'cancel':
                        thread = threading.Thread(target=sampler.run, daemon=True)
                        thread.start()
                        deadline = time.monotonic() + 1
                        while not ready.exists() and time.monotonic() < deadline:
                            time.sleep(.01)
                        if not ready.exists():
                            raise telemetry.TelemetryError('inventory helper readiness failure')
                        sampler.stop.set()
                        thread.join(timeout=2)
                        if thread.is_alive() or sampler.error is not None:
                            raise telemetry.TelemetryError('cancelled inventory did not stop cleanly')
                    elif case == 'rss':
                        sampler.sample()
                        if sampler.peak_rss != 18 * 1024:
                            raise telemetry.TelemetryError('process-tree RSS observation changed')
                    elif case == 'exact':
                        if sampler.darwin_inventory() != ' ' * (1024 * 1024):
                            raise telemetry.TelemetryError('exact-bound inventory was truncated')
                    elif case == 'descendant':
                        if sampler.darwin_inventory() != '':
                            raise telemetry.TelemetryError('exited inventory leader observation changed')
                    elif case == 'timeout':
                        sampler.run()
                        if sampler.error != 'process inventory exceeded its five-second deadline':
                            raise telemetry.TelemetryError('inventory deadline failure was not propagated')
                    else:
                        expected = {
                            'oversize': 'process inventory exceeds its 1 MiB output bound',
                            'nonzero': 'process inventory exited unsuccessfully',
                            'nonascii': 'process inventory is not ASCII',
                        }[case]
                        try:
                            sampler.darwin_inventory()
                        except telemetry.TelemetryError as exc:
                            if str(exc) != expected:
                                raise telemetry.TelemetryError(f'{case}: wrong rejection: {exc}') from exc
                        else:
                            raise telemetry.TelemetryError(f'{case}: invalid inventory accepted')
                if len(helpers) != 1:
                    raise telemetry.TelemetryError('inventory restarted after stop or failure')
                helper = helpers[0]
                if helper.returncode is None or helper.stdout is None or not helper.stdout.closed:
                    raise telemetry.TelemetryError('inventory helper or pipe survived cleanup')
                deadline = time.monotonic() + 2
                while True:
                    try:
                        os.killpg(helper.pid, 0)
                    except ProcessLookupError:
                        break
                    except PermissionError:
                        # Darwin can retain an unsignalable zombie group while
                        # launchd reaps a killed orphan. Require disappearance;
                        # never count permission denial as successful cleanup.
                        pass
                    if time.monotonic() >= deadline:
                        raise telemetry.TelemetryError('inventory process group survived cleanup')
                    time.sleep(.01)
                if case == 'descendant' and int(child_ready.read_text()) != helper.pid:
                    raise telemetry.TelemetryError('inventory descendant escaped the owned group')
                # A helper's private group cannot be the governed caller's group.
                os.kill(os.getpid(), 0)
                controls += 1
            except BaseException:
                print(f'inventory control failed: case={case} helpers={[h.pid for h in helpers]}', file=sys.stderr)
                raise
            finally:
                sampler.stop.set()
                for helper in helpers:
                    try:
                        os.killpg(helper.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    helper.wait(timeout=1)
                    if helper.stdout is not None:
                        helper.stdout.close()
                if thread is not None:
                    thread.join(timeout=2)
        # Stopping before launch does not issue a final inventory request.
        sampler = telemetry.Sampler(os.getpid(), 20)
        sampler.platform = 'darwin'
        sampler.stop.set()
        with patch.object(telemetry.subprocess, 'Popen', side_effect=AssertionError('work after stop')):
            sampler.run()
        if sampler.error is not None:
            raise telemetry.TelemetryError('pre-stopped sampler failed')
        controls += 1
    print(f'gate-telemetry-sampler: ok (controls={controls} helpers_reaped=8 pipes_closed=8 descendant_group_drained=1)')
    return controls


def inventory_protocol_self_test() -> int:
    pid = os.getpid()
    invalid = [
        'garbage\n', f'{pid} 0 ' + '9' * 5000 + '\n',
        f'{pid} 0 -1\n', f'{pid} 0 {2**54 + 1}\n',
        '0 0 1\n', f'{2**31} 0 1\n',
        f'{pid} 0 1\n{pid} 0 1\n',
        f'{pid} 0 1\n900001 900002 1\n',
        f'{pid} 0 1 extra\n',
    ]
    for payload in invalid:
        sampler = telemetry.Sampler(pid, 20)
        sampler.platform = 'darwin'
        sampler.darwin_inventory = lambda: payload
        sampler.run()
        if sampler.error is None or sampler.peak_rss != 0:
            raise telemetry.TelemetryError('malformed inventory row escaped its sealed observation error')
    print(f'gate-telemetry-inventory-protocol: ok (negative_controls={len(invalid)})')
    return len(invalid)


def native_inventory_self_test() -> int:
    module = telemetry.darwin_inventory
    controls = 0
    class Tree:
        def __init__(self, children=None, resident=1):
            self.links = children if children is not None else {1: [2, 2], 2: [1]}
            self.rss = resident
        def resident(self, pid):
            return self.rss
        def children(self, pid):
            return self.links.get(pid, [])
    if module.ctypes.sizeof(module.RusageInfoV0) != 96 or module.RusageInfoV0.ri_resident_size.offset != 64:
        raise telemetry.TelemetryError('RUSAGE_INFO_V0 ABI layout drift')
    if list(module.rows(1, Tree())) != ['1 0 1\n', '2 1 1\n']:
        raise telemetry.TelemetryError('inventory cycle/deduplication/rounding control failed')
    controls += 1
    def reject(root, tree, expected):
        try:
            list(module.rows(root, tree))
        except module.InventoryError as exc:
            if str(exc) != expected:
                raise telemetry.TelemetryError(f'wrong native inventory rejection: {exc}') from exc
        else:
            raise telemetry.TelemetryError('native inventory negative was accepted')
    for root in (True, -1, 2**31, '1'):
        reject(root, Tree(), 'process root PID is invalid')
        controls += 1
    for resident in (True, -1, 2**64):
        reject(1, Tree(resident=resident), 'process resident size is invalid')
        controls += 1
    for child in (True, 0, 2**31):
        reject(1, Tree(children={1: [child]}), 'process-child inventory contains an invalid PID')
        controls += 1
    with patch.object(module, 'MAX_PROCESSES', 2):
        reject(1, Tree(children={1: [2, 3]}), 'process inventory exceeds its count bound')
    controls += 1
    with patch.object(module, 'MAX_OUTPUT_BYTES', 5):
        reject(1, Tree(), 'process inventory exceeds its output bound')
    controls += 1
    class Library:
        def __init__(self, listing, rusage=None):
            self.proc_listpids = listing
            self.proc_pid_rusage = rusage
    native = object.__new__(module.NativeInventory)
    def listing(result, error=0):
        def call(kind, pid, buffer, size):
            module.ctypes.set_errno(error)
            return result if result is not None else size + 4
        return call
    for result, error, expected in (
        (0, 0, []), (0, module.errno.ESRCH, []),
        (0, module.errno.EACCES, None), (1, 0, None), (None, 0, None),
    ):
        native.library = Library(listing(result, error))
        try:
            actual = native.children(1)
        except module.InventoryError:
            if expected is not None:
                raise
        else:
            if expected is None or actual != expected:
                raise telemetry.TelemetryError('invalid native child inventory accepted')
        controls += 1
    calls = []
    def exact(kind, pid, buffer, size):
        module.ctypes.set_errno(0)
        calls.append(size)
        for index in range(64):
            buffer[index] = index + 1
        return 64 * module.ctypes.sizeof(module.ctypes.c_int)
    native.library = Library(exact)
    if native.children(1) != list(range(1, 65)) or calls != [256, 512]:
        raise telemetry.TelemetryError('full native child buffer was silently truncated')
    controls += 1
    for error, expected in ((module.errno.ESRCH, None), (module.errno.EACCES, 'error')):
        def rusage(pid, flavor, buffer):
            module.ctypes.set_errno(error)
            return -1
        native.library = Library(None, rusage)
        try:
            actual = native.resident(1)
        except module.InventoryError:
            if expected != 'error':
                raise
        else:
            if expected == 'error' or actual is not None:
                raise telemetry.TelemetryError('native rusage failure classification changed')
        controls += 1
    class Group:
        def __init__(self, info):
            self.info = info
        def pids(self, kind, pgid):
            if kind != 2 or pgid != 1:
                raise telemetry.TelemetryError('native group selector drift')
            return [2]
        def resource_info(self, pid):
            return self.info
    for state, expected in ((None, False), (0, True), (1, False)):
        info = None if state is None else module.RusageInfoV0()
        if info is not None:
            info.ri_proc_exit_abstime = state
        if module.NativeInventory.group_has_live_processes(Group(info), 1) != expected:
            raise telemetry.TelemetryError('live/zombie native group classification changed')
        controls += 1
    def denied(self, pid):
        raise module.InventoryError('denied group member observation')
    with patch.object(Group, 'resource_info', denied):
        try:
            module.NativeInventory.group_has_live_processes(Group(None), 1)
        except module.InventoryError:
            pass
        else:
            raise telemetry.TelemetryError('unknown group state accepted as dead')
    controls += 1
    sampler = telemetry.Sampler(os.getpid(), 20)
    def failed_native_sample():
        raise module.InventoryError('unknown native group state')
    sampler.sample = failed_native_sample
    sampler.run()
    if sampler.error != 'unknown native group state':
        raise telemetry.TelemetryError('native inventory failure disappeared in sampler thread')
    controls += 1
    module.ctypes.set_errno(0)
    if sys.platform == 'darwin':
        popen = subprocess.Popen
        killpg = os.killpg
        native = module.NativeInventory()
        for fault in ('honest', 'live', 'unknown'):
            sampler = telemetry.Sampler(os.getpid(), 20)
            helpers = []
            def exited_spawn(argv, **kwargs):
                proc = popen([sys.executable, '-c', 'pass'], **kwargs)
                helpers.append(proc)
                deadline = time.monotonic() + 1
                while time.monotonic() < deadline:
                    info = module.RusageInfoV0()
                    if native.library.proc_pid_rusage(proc.pid, 0, module.ctypes.byref(info)) == 0 and info.ri_proc_exit_abstime:
                        sampler.stop.set()
                        return proc
                    time.sleep(.01)
                raise telemetry.TelemetryError('exited helper fixture readiness failed')
            def denied_group(pgid, sig):
                if helpers and pgid == helpers[0].pid:
                    raise PermissionError('injected signal denial')
                return killpg(pgid, sig)
            try:
                with patch.object(telemetry.subprocess, 'Popen', exited_spawn):
                    if fault == 'honest':
                        if sampler.darwin_inventory() is not None:
                            raise telemetry.TelemetryError('stopped, exited helper observation changed')
                    else:
                        kwargs = ({'return_value': True} if fault == 'live' else
                                  {'side_effect': module.InventoryError('unknown group state')})
                        with patch.object(telemetry.os, 'killpg', denied_group), patch.object(module.NativeInventory, 'group_has_live_processes', **kwargs):
                            try:
                                sampler.darwin_inventory()
                            except (telemetry.TelemetryError, module.InventoryError) as exc:
                                expected = ('inventory group signal denied (leader_status=0)' if fault == 'live' else 'unknown group state')
                                if str(exc) != expected:
                                    raise telemetry.TelemetryError('wrong exited-group rejection') from exc
                            else:
                                raise telemetry.TelemetryError('live/unknown group accepted as dead')
                if len(helpers) != 1 or helpers[0].returncode != 0 or not helpers[0].stdout.closed:
                    raise telemetry.TelemetryError('exited helper fixture leaked owner resources')
            finally:
                for proc in helpers:
                    if proc.poll() is None:
                        killpg(proc.pid, signal.SIGKILL)
                    proc.wait(timeout=1)
                    if proc.stdout is not None:
                        proc.stdout.close()
        print('gate-telemetry-native-inventory: actual exited-leader control and live/unknown-state faults passed (E0)')
        with tempfile.TemporaryDirectory(prefix='telemetry-native-inventory-') as temporary:
            ready = Path(temporary) / 'ready'
            code = ('import os,time;from pathlib import Path; allocation=bytearray(4*1024*1024); '
                    f'Path({str(ready)!r}).write_text(str(os.getpid())); time.sleep(30)')
            proc = subprocess.Popen([sys.executable, '-c', code], start_new_session=True)
            try:
                deadline = time.monotonic() + 1
                while not ready.exists() and time.monotonic() < deadline:
                    time.sleep(.01)
                if not ready.exists():
                    raise telemetry.TelemetryError('native process fixture readiness failed')
                sampler = telemetry.Sampler(os.getpid(), 20)
                text = sampler.darwin_inventory()
                records = {int(row[0]): int(row[2]) for row in (line.split() for line in text.splitlines())}
                if os.getpid() not in records or records.get(proc.pid, 0) < 4096:
                    raise telemetry.TelemetryError('actual libproc inventory lost the live child allocation')
            finally:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait(timeout=1)
        print('gate-telemetry-native-inventory: actual Darwin parent/child RSS control passed (E0)')
    else:
        print('gate-telemetry-native-inventory: Darwin runtime not qualified on this host')
    print(f'gate-telemetry-native-inventory: ok (contract_controls={controls})')
    return controls


def birth_identity_self_test() -> int:
    import copy
    import generated_state as state
    module = telemetry.darwin_inventory
    controls = 0
    native = object.__new__(module.NativeInventory)
    class Library:
        pass
    for result, error, reported_pid, sec, usec, expected in (
        (136, 0, 1, 1700000000, 7, 1700000000),
        (0, module.errno.ESRCH, 0, 0, 0, None),
        (135, 0, 1, 1700000000, 7, 'error'),
        (136, module.errno.EACCES, 1, 1700000000, 7, 'error'),
        (136, 0, 2, 1700000000, 7, 'error'),
        (136, 0, 1, 0, 7, 'error'),
        (136, 0, 1, 1700000000, 1000000, 'error'),
    ):
        def read(pid, flavor, arg, pointer, size):
            if flavor != 3 or arg != 0 or size != 136:
                raise telemetry.TelemetryError('native birth ABI selector drift')
            info = module.ctypes.cast(pointer, module.ctypes.POINTER(module.BsdInfo)).contents
            info.pid, info.sec, info.usec = reported_pid, sec, usec
            module.ctypes.set_errno(error)
            return result
        native.library = Library()
        native.library.proc_pidinfo = read
        try:
            actual = native.start_time(1)
        except module.InventoryError:
            if expected != 'error':
                raise
        else:
            if expected == 'error' or actual != expected:
                raise telemetry.TelemetryError('native birth negative was accepted')
        controls += 1
    original_is_file = Path.is_file
    def is_file(path):
        return False if path == Path(f'/proc/{os.getpid()}/stat') else original_is_file(path)
    with patch.object(state.sys, 'platform', 'darwin'), patch.object(Path, 'is_file', is_file), patch.object(module.NativeInventory, '__init__', lambda self: None), patch.object(module.NativeInventory, 'start_time', return_value=1700000000), patch.object(state.subprocess, 'run', side_effect=AssertionError('privileged PID observer launched')):
        expected = state.digest_bytes(f'posix\0{os.getpid()}\0{time.asctime(time.localtime(1700000000))}'.encode())
        if state.process_identity(os.getpid()) != expected:
            raise telemetry.TelemetryError('legacy PID birth digest format changed')
        controls += 1
        registry = {'leases': [{'pid': os.getpid(), 'processIdentity': expected, 'entryId': 'fixture'}], 'entries': []}
        before = copy.deepcopy(registry)
        with patch.object(module.NativeInventory, 'start_time', side_effect=module.InventoryError('denied birth observation')):
            try:
                state._recover_leases(Path.cwd(), registry, state.process_identity)
            except state.GeneratedStateError:
                pass
            else:
                raise telemetry.TelemetryError('unknown birth observation was reclassified as idle')
        if registry != before:
            raise telemetry.TelemetryError('unknown birth observation changed lease custody')
        controls += 1
    module.ctypes.set_errno(0)
    print(f'gate-telemetry-birth-identity: ok (controls={controls} legacy_format_preserved=1 unknown_lease_preserved=1)')
    return controls


if __name__ == '__main__':
    sampler_self_test()
    native_inventory_self_test()
    inventory_protocol_self_test()
    birth_identity_self_test()
