#!/usr/bin/env python3
import os
import sys


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(2)


header = sys.stdin.buffer.readline(32)
if not header.endswith(b"\n") or not header[:-1].isdigit():
    fail("invalid bridge frame header")
size = int(header[:-1])
if size > 65536:
    fail("bridge request exceeds fixture limit")
request = sys.stdin.buffer.read(size)
if len(request) != size or sys.stdin.buffer.read(1):
    fail("invalid bridge frame body")
if os.environ.get("GENESIS_HOST_BRIDGE_OP") != "host/plugin::command":
    fail("unexpected bridge operation")
required = (
    b'genesis.agent-model-runner.v0.1',
    b':command "infer"',
    b':model-id "genesis-agent-fixture"',
)
if any(value not in request for value in required):
    fail("request does not match the bound model profile")
response = (
    b'{:finish-reason "stop" :model-id "genesis-agent-fixture" '
    b':model-revision "sha256:ea5ae5d06096510cc7eb945a16687f2dfce0e2f1edbc4001ab1c4c07b24be5dd" :ok true '
    b':output "(prim int/add 40 2)\\n" '
    b':usage {:input-tokens 96 :output-tokens 7}}'
)
sys.stdout.buffer.write(str(len(response)).encode("ascii") + b"\n" + response)
