#!/usr/bin/env python3
"""Bounded observations of allocated file blocks in caller-owned roots."""
from __future__ import annotations

import os
from pathlib import Path
import stat
from typing import Sequence

from supervisor_cancellation import deferred_cancellation

_DESCRIPTOR_BACKEND_AVAILABLE = (
    hasattr(os, "O_DIRECTORY") and hasattr(os, "O_NOFOLLOW")
    and os.stat in os.supports_dir_fd and os.stat in os.supports_follow_symlinks
    and os.scandir in os.supports_fd
)


class AllocationError(ValueError):
    pass


def allocated_paths_bytes(
    paths: Sequence[Path], *, max_entries: int = 2_000_000,
    max_depth: int = 256, reject_symlinks: bool = False,
) -> int:
    """Sample each inode once; hold directory descriptors during descent.

    A disappearing name is an ordinary concurrent-writer observation. Unknown
    metadata is an error. Sparse logical length is not allocated disk space.
    Callers authorize roots; this observation does not grant deletion authority.
    """
    if (isinstance(max_entries, bool) or not isinstance(max_entries, int) or max_entries <= 0
            or isinstance(max_depth, bool) or not isinstance(max_depth, int) or max_depth <= 0):
        raise AllocationError("allocation observation bounds must be positive integers")
    if not _DESCRIPTOR_BACKEND_AVAILABLE:
        raise AllocationError("descriptor allocation observation is unsupported on this host")
    if len(paths) > max_entries:
        raise AllocationError("allocation observation exceeds its entry bound")
    allocations = {}
    opened_directories = set()
    frames = []
    entries = 0

    def count_entry():
        nonlocal entries
        entries += 1
        if entries > max_entries:
            raise AllocationError("allocation observation exceeds its entry bound")

    def charge(metadata):
        fields = [getattr(metadata, name, None) for name in ("st_dev", "st_ino", "st_blocks", "st_mode")]
        if (any(isinstance(value, bool) or not isinstance(value, int) or value < 0 for value in fields)
                or fields[1] == 0):
            raise AllocationError("allocated-block metadata is unavailable or invalid")
        if reject_symlinks and stat.S_ISLNK(fields[3]):
            raise AllocationError("allocation observation contains a symlink")
        identity = (fields[0], fields[1])
        # A writer can grow the same inode between two alias observations.
        allocations[identity] = max(allocations.get(identity, 0), fields[2] * 512)
        return identity

    def mode(metadata):
        value = getattr(metadata, "st_mode", None)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise AllocationError("allocated-block metadata is unavailable or invalid")
        return value

    def directory(name, depth, parent_fd=None):
        if depth > max_depth:
            raise AllocationError("allocation observation exceeds its directory-depth bound")
        descriptor = None
        iterator = None
        try:
            with deferred_cancellation():
                descriptor = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent_fd)
            identity = charge(os.fstat(descriptor))
            if identity in opened_directories:
                return
            opened_directories.add(identity)
            with deferred_cancellation():
                iterator = os.scandir(descriptor)
                frames.append((descriptor, iterator, depth))
                descriptor = None
                iterator = None
        finally:
            with deferred_cancellation():
                try:
                    if iterator is not None:
                        iterator.close()
                finally:
                    if descriptor is not None:
                        os.close(descriptor)

    try:
        for root in paths:
            count_entry()
            try:
                metadata = root.lstat()
                if stat.S_ISDIR(mode(metadata)):
                    directory(root, 1)
                else:
                    charge(metadata)
            except FileNotFoundError:
                continue
            while frames:
                descriptor, iterator, depth = frames[-1]
                try:
                    entry = next(iterator)
                except (StopIteration, FileNotFoundError):
                    with deferred_cancellation():
                        frames.pop()
                        try:
                            iterator.close()
                        finally:
                            os.close(descriptor)
                    continue
                count_entry()
                try:
                    metadata = os.stat(entry.name, dir_fd=descriptor, follow_symlinks=False)
                    if stat.S_ISDIR(mode(metadata)):
                        directory(entry.name, depth + 1, descriptor)
                    else:
                        charge(metadata)
                except FileNotFoundError:
                    continue
        return sum(allocations.values())
    except OSError as exc:
        raise AllocationError(f"allocation metadata is unreadable (errno={exc.errno})") from exc
    finally:
        with deferred_cancellation():
            for descriptor, iterator, _ in reversed(frames):
                try:
                    iterator.close()
                finally:
                    os.close(descriptor)
