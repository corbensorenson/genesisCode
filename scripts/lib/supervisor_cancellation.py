#!/usr/bin/env python3
"""Shared one-shot signal unwinding and protected ownership transfer."""
from __future__ import annotations

from contextlib import contextmanager
import signal
from typing import Any, Optional

class SupervisorCancelled(BaseException):
    def __init__(self, signum: int):
        self.signum = signum


class CancellationScope:
    """Unwind once; repeated termination must not interrupt owned cleanup."""

    def __init__(self) -> None:
        self.depth = 0
        self.pending: Optional[int] = None
        self.unwinding = False

    def receive(self, signum: int, _frame: Any) -> None:
        if self.unwinding:
            return
        if self.pending is None:
            self.pending = signum
        self.deliver()

    def deliver(self) -> None:
        if self.pending is not None and self.depth == 0 and not self.unwinding:
            self.unwinding = True
            raise SupervisorCancelled(self.pending)

    @contextmanager
    def defer(self):
        self.depth += 1
        try:
            yield
        finally:
            self.depth -= 1
            self.deliver()


_cancellation_scope: Optional[CancellationScope] = None


@contextmanager
def cancellation_scope():
    global _cancellation_scope
    previous_scope = _cancellation_scope
    scope = CancellationScope()
    handlers = {}
    try:
        for name in ("SIGINT", "SIGTERM", "SIGHUP"):
            signum = getattr(signal, name, None)
            if signum is not None:
                handlers[signum] = signal.signal(signum, scope.receive)
        _cancellation_scope = scope
        yield
    finally:
        _cancellation_scope = previous_scope
        for signum, handler in handlers.items():
            signal.signal(signum, handler)


@contextmanager
def deferred_cancellation():
    if _cancellation_scope is None:
        yield
    else:
        with _cancellation_scope.defer():
            yield


