#!/usr/bin/env python3
"""Unprivileged, bounded libproc process-tree inventory for the Darwin sampler."""
from __future__ import annotations

from collections import deque
import ctypes
import errno
import sys

MAX_PROCESSES = 32768
MAX_OUTPUT_BYTES = 1024 * 1024


class InventoryError(ValueError):
    pass


class BsdInfo(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint32) for name in (
        'flags', 'status', 'xstatus', 'pid', 'ppid', 'uid', 'gid', 'ruid',
        'rgid', 'svuid', 'svgid', 'reserved',
    )] + [('comm', ctypes.c_char * 16), ('name', ctypes.c_char * 32)] + [
        (name, ctypes.c_uint32) for name in ('nfiles', 'pgid', 'jobc', 'tdev', 'tpgid')
    ] + [('nice', ctypes.c_int32), ('sec', ctypes.c_uint64), ('usec', ctypes.c_uint64)]


class RusageInfoV0(ctypes.Structure):
    # The public RUSAGE_INFO_V0 ABI in sys/resource.h: UUID then ten uint64s.
    _fields_ = [('ri_uuid', ctypes.c_uint8 * 16)] + [
        (name, ctypes.c_uint64) for name in (
            'ri_user_time', 'ri_system_time', 'ri_pkg_idle_wkups',
            'ri_interrupt_wkups', 'ri_pageins', 'ri_wired_size',
            'ri_resident_size', 'ri_phys_footprint',
            'ri_proc_start_abstime', 'ri_proc_exit_abstime',
        )
    ]


class NativeInventory:
    def __init__(self):
        if (sys.platform != 'darwin' or ctypes.sizeof(RusageInfoV0) != 96
                or ctypes.sizeof(BsdInfo) != 136 or BsdInfo.sec.offset != 120):
            raise InventoryError('Darwin resource ABI is unavailable')
        self.library = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
        self.library.proc_listpids.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_int]
        self.library.proc_listpids.restype = ctypes.c_int
        self.library.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
        self.library.proc_pid_rusage.restype = ctypes.c_int
        self.library.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
        self.library.proc_pidinfo.restype = ctypes.c_int

    def start_time(self, pid):
        info = BsdInfo()
        ctypes.set_errno(0)
        size = ctypes.sizeof(info)
        received = self.library.proc_pidinfo(pid, 3, 0, ctypes.byref(info), size)
        error = ctypes.get_errno()
        if received == 0 and error == errno.ESRCH:
            return None
        if (error or received != size or info.pid != pid or info.sec == 0
                or info.usec >= 1000000):
            raise InventoryError(f'process birth inventory failed (errno={error})')
        return int(info.sec)

    def pids(self, kind, pid):
        capacity = 64
        while capacity <= MAX_PROCESSES:
            buffer = (ctypes.c_int * capacity)()
            size = ctypes.sizeof(buffer)
            ctypes.set_errno(0)
            received = self.library.proc_listpids(kind, pid, buffer, size)
            error = ctypes.get_errno()
            if received == 0 and error == errno.ESRCH:
                return []
            if error or received < 0 or received > size or received % ctypes.sizeof(ctypes.c_int):
                raise InventoryError(f'process-child inventory failed (errno={error})')
            if received < size:
                pids = list(buffer[:received // ctypes.sizeof(ctypes.c_int)])
                if any(pid <= 0 for pid in pids):
                    raise InventoryError('process-child inventory contains an invalid PID')
                return pids
            capacity *= 2
        raise InventoryError('process-child inventory exceeds its count bound')

    def children(self, pid):
        return self.pids(6, pid)  # PROC_PPID_ONLY

    def resource_info(self, pid):
        info = RusageInfoV0()
        ctypes.set_errno(0)
        result = self.library.proc_pid_rusage(pid, 0, ctypes.byref(info))
        error = ctypes.get_errno()
        if result != 0:
            if error == errno.ESRCH:
                return None  # An exited process is an ordinary sampled race.
            raise InventoryError(f'process resource inventory failed (errno={error})')
        return info

    def resident(self, pid):
        info = self.resource_info(pid)
        return None if info is None else int(info.ri_resident_size)

    def group_has_live_processes(self, pgid):
        for pid in self.pids(2, pgid):  # PROC_PGRP_ONLY
            info = self.resource_info(pid)
            if info is not None and info.ri_proc_exit_abstime == 0:
                return True
        return False


def rows(root_pid, inventory):
    if isinstance(root_pid, bool) or not isinstance(root_pid, int) or not 0 < root_pid <= 2**31 - 1:
        raise InventoryError('process root PID is invalid')
    queue = deque([(root_pid, 0)])
    discovered = {root_pid}
    output_bytes = 0
    while queue:
        pid, parent = queue.popleft()
        resident = inventory.resident(pid)
        if resident is None:
            continue
        if isinstance(resident, bool) or not isinstance(resident, int) or not 0 <= resident <= 2**64 - 1:
            raise InventoryError('process resident size is invalid')
        line = f'{pid} {parent} {(resident + 1023) // 1024}\n'
        output_bytes += len(line.encode('ascii'))
        if output_bytes > MAX_OUTPUT_BYTES:
            raise InventoryError('process inventory exceeds its output bound')
        yield line
        for child in inventory.children(pid):
            if isinstance(child, bool) or not isinstance(child, int) or not 0 < child <= 2**31 - 1:
                raise InventoryError('process-child inventory contains an invalid PID')
            if child not in discovered:
                if len(discovered) >= MAX_PROCESSES:
                    raise InventoryError('process inventory exceeds its count bound')
                discovered.add(child)
                queue.append((child, pid))


def main():
    if len(sys.argv) != 2 or len(sys.argv[1]) > 10 or not sys.argv[1].isascii() or not sys.argv[1].isdigit():
        raise InventoryError('process root PID is invalid')
    for line in rows(int(sys.argv[1]), NativeInventory()):
        sys.stdout.write(line)


if __name__ == '__main__':
    try:
        main()
    except (InventoryError, OSError) as exc:
        print(f'process inventory: {exc}', file=sys.stderr)
        raise SystemExit(2)
