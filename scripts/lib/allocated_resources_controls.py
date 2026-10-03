#!/usr/bin/env python3
"""Filesystem and fault controls for bounded allocated-block observations."""
from __future__ import annotations

import errno
import os
import signal
from pathlib import Path
import stat
import tempfile
from types import SimpleNamespace
from unittest.mock import patch

import allocated_resources as allocation
from supervisor_cancellation import SupervisorCancelled, cancellation_scope


def allocation_self_test() -> int:
    controls = []

    def require(value, message):
        if not value:
            raise AssertionError(f"allocated-resources: {message}")

    def rejected(function, diagnostic):
        try:
            function()
        except allocation.AllocationError as exc:
            require(diagnostic in str(exc), f"wrong diagnostic: {exc}")
        else:
            raise AssertionError(f"allocated-resources: accepted {diagnostic}")

    def blocks(path):
        return path.lstat().st_blocks * 512

    with tempfile.TemporaryDirectory(prefix="allocated-resources-controls-") as temporary:
        root = Path(temporary).resolve()
        scope = root / "owned"; scope.mkdir()
        file = scope / "artifact"; file.write_bytes(b"x" * 65536)
        alias = scope / "alias"; os.link(file, alias)
        expected = blocks(scope) + blocks(file)
        require(allocation.allocated_paths_bytes([scope, file, alias, scope]) == expected,
                "hard links or overlapping roots were charged twice")
        controls.append("native-hardlink-and-overlap-identity")
        alias.unlink(); file.unlink()

        with file.open("wb") as handle:
            handle.truncate(1 << 30)
        require(allocation.allocated_paths_bytes([scope]) == blocks(scope) + blocks(file),
                "sparse logical length became allocated bytes")
        require(blocks(file) < file.stat().st_size, "sparse control is not sparse")
        controls.append("native-sparse-allocation")
        file.unlink()
        require(allocation.allocated_paths_bytes([scope / "absent"]) == 0, "missing root observation changed")
        controls.append("missing-root")
        for value in (True, 0, -1, "1"):
            rejected(lambda: allocation.allocated_paths_bytes([scope], max_entries=value), "bounds")
            rejected(lambda: allocation.allocated_paths_bytes([scope], max_depth=value), "bounds")
        controls.append("closed-observation-bounds")

        file.write_bytes(b"x")
        actual_stat = os.stat
        def disappears(name, *args, **kwargs):
            if name == "artifact" and kwargs.get("dir_fd") is not None:
                file.unlink()
                raise FileNotFoundError(errno.ENOENT, "controlled disappearance")
            return actual_stat(name, *args, **kwargs)
        with patch.object(allocation.os, "stat", disappears):
            require(allocation.allocated_paths_bytes([scope]) == blocks(scope), "disappearing file was an unknown error")
        controls.append("file-disappearance-race")

        directory = scope / "directory"; directory.mkdir()
        actual_open = os.open
        def directory_disappears(name, *args, **kwargs):
            if name == "directory" and kwargs.get("dir_fd") is not None:
                directory.rmdir()
                raise FileNotFoundError(errno.ENOENT, "controlled directory disappearance")
            return actual_open(name, *args, **kwargs)
        with patch.object(allocation.os, "open", directory_disappears):
            require(allocation.allocated_paths_bytes([scope]) == blocks(scope), "disappearing directory was an unknown error")
        controls.append("directory-disappearance-race")

        file.write_bytes(b"x")
        def unreadable(name, *args, **kwargs):
            if name == "artifact" and kwargs.get("dir_fd") is not None:
                raise PermissionError(errno.EACCES, "controlled unreadable metadata")
            return actual_stat(name, *args, **kwargs)
        with patch.object(allocation.os, "stat", unreadable):
            rejected(lambda: allocation.allocated_paths_bytes([scope]), f"errno={errno.EACCES}")
        controls.append("unknown-metadata-fails-closed")
        with patch.object(allocation.os, "scandir", side_effect=PermissionError(errno.EACCES, "controlled enumeration denial")):
            rejected(lambda: allocation.allocated_paths_bytes([scope]), f"errno={errno.EACCES}")
        controls.append("unknown-enumeration-fails-closed")

        real = file.stat()
        for value in (None, True, -1, "1"):
            fake = SimpleNamespace(st_dev=real.st_dev, st_ino=real.st_ino, st_mode=real.st_mode, st_blocks=value)
            with patch.object(Path, "lstat", return_value=fake):
                rejected(lambda: allocation.allocated_paths_bytes([file]), "metadata is unavailable or invalid")
        with patch.object(allocation, "_DESCRIPTOR_BACKEND_AVAILABLE", False):
            rejected(lambda: allocation.allocated_paths_bytes([scope]), "unsupported")
        controls.append("missing-or-invalid-allocation-backend")

        outside = root / "outside"; outside.mkdir()
        victim = outside / "victim"; victim.write_bytes(b"v" * (128 * 1024))
        link = scope / "link"; link.symlink_to(outside, target_is_directory=True)
        rejected(lambda: allocation.allocated_paths_bytes([scope], reject_symlinks=True), "symlink")
        link.unlink(); link.symlink_to(root / "missing")
        rejected(lambda: allocation.allocated_paths_bytes([link], reject_symlinks=True), "symlink")
        controls.append("strict-honest-and-dangling-link-rejection")
        require(allocation.allocated_paths_bytes([scope]) == blocks(scope) + blocks(file) + blocks(link),
                "non-following observation charged a link destination")
        controls.append("link-entry-observation-without-traversal")
        link.unlink()

        directory.mkdir(); parked = root / "parked"
        outside_seen = []
        actual_scandir = os.scandir
        def replace_directory(name, *args, **kwargs):
            if name == "directory" and kwargs.get("dir_fd") is not None:
                directory.rename(parked); directory.symlink_to(outside, target_is_directory=True)
            return actual_open(name, *args, **kwargs)
        def observe_directory(descriptor):
            outside_seen.append(os.fstat(descriptor).st_ino == outside.stat().st_ino)
            return actual_scandir(descriptor)
        with patch.object(allocation.os, "open", replace_directory), patch.object(allocation.os, "scandir", observe_directory):
            rejected(lambda: allocation.allocated_paths_bytes([scope]), "unreadable")
        require(not any(outside_seen) and victim.read_bytes() == b"v" * (128 * 1024),
                "directory replacement escaped a held parent")
        directory.unlink(); parked.rmdir()
        controls.append("directory-replacement-does-not-follow-link")

        rejected(lambda: allocation.allocated_paths_bytes([scope], max_entries=1), "entry bound")
        directory.mkdir()
        rejected(lambda: allocation.allocated_paths_bytes([scope], max_depth=1), "directory-depth bound")
        controls.append("finite-entry-and-depth-cost")

        opened = []
        def record_open(*args, **kwargs):
            descriptor = actual_open(*args, **kwargs); opened.append(descriptor); return descriptor
        with patch.object(allocation.os, "open", record_open):
            allocation.allocated_paths_bytes([scope])
            rejected(lambda: allocation.allocated_paths_bytes([scope], max_entries=1), "entry bound")
            with patch.object(allocation.os, "scandir", side_effect=PermissionError(errno.EACCES, "controlled denial")):
                rejected(lambda: allocation.allocated_paths_bytes([scope]), "unreadable")
        for descriptor in opened:
            try:
                os.fstat(descriptor)
            except OSError as exc:
                require(exc.errno == errno.EBADF, "wrong descriptor terminal state")
            else:
                raise AssertionError("allocated-resources: directory descriptor leaked")
        controls.append("descriptors-close-on-success-and-fault")

        for phase in ("open", "iterator"):
            opened = []
            iterators = []
            def interrupt_open(*args, **kwargs):
                descriptor = actual_open(*args, **kwargs); opened.append(descriptor)
                if phase == "open":
                    os.kill(os.getpid(), signal.SIGTERM)
                return descriptor
            def interrupt_iterator(*args, **kwargs):
                iterator = actual_scandir(*args, **kwargs); iterators.append(iterator)
                os.kill(os.getpid(), signal.SIGTERM)
                return iterator
            try:
                with cancellation_scope(), patch.object(allocation.os, "open", interrupt_open):
                    if phase == "iterator":
                        with patch.object(allocation.os, "scandir", interrupt_iterator):
                            allocation.allocated_paths_bytes([scope])
                    else:
                        allocation.allocated_paths_bytes([scope])
            except SupervisorCancelled as exc:
                require(exc.signum == signal.SIGTERM, "wrong ownership-transfer cancellation")
            else:
                raise AssertionError("allocated-resources: cancellation was not delivered")
            for descriptor in opened:
                try:
                    os.fstat(descriptor)
                except OSError as exc:
                    require(exc.errno == errno.EBADF, "cancelled descriptor has unknown terminal state")
                else:
                    raise AssertionError("allocated-resources: cancelled descriptor leaked")
            for iterator in iterators:
                require(next(iterator, None) is None, "cancelled iterator survived ownership unwind")
            controls.append(f"cancelled-{phase}-ownership-transfer")

    print(f"allocated-resources: ok (control_groups={len(controls)} no_logical_fallback=1 held_descent=1)")
    return len(controls)


if __name__ == "__main__":
    allocation_self_test()
