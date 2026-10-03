#!/usr/bin/env python3
"""Finite public controls for bounded Cargo metadata operation admission."""
from __future__ import annotations

import contextlib
from concurrent.futures import ThreadPoolExecutor
import copy
import errno
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from unittest.mock import patch

import cargo_cache as cache
import generated_state as state


def require(value, message):
    if not value:
        raise AssertionError(f"cargo-metadata-admission: {message}")


def rejected(function, diagnostic):
    try:
        function()
    except (state.GeneratedStateError, cache.CachePolicyError) as exc:
        require(diagnostic in str(exc), f"wrong diagnostic: {exc}")
    else:
        raise AssertionError(f"accepted negative control: {diagnostic}")


def metadata_self_test(source_root: Path) -> int:
    controls = []
    with tempfile.TemporaryDirectory(prefix="cargo-metadata-admission-") as temporary:
        root = Path(temporary).resolve()
        files = {
            cache.POLICY_REL, cache.SCHEMA_REL, state.POLICY_REL,
            "policies/deterministic_cleanup_v0.1.json", "rust-toolchain.toml",
            ".cargo/config.toml", "Cargo.lock", "tools/genesis-evidence-verifier/Cargo.lock",
            "scripts/lib/cargo_cache.py", "scripts/lib/generated_state.py",
            "scripts/lib/deterministic_cleanup.py",
        }
        policy = cache.load_policy(source_root)
        for scope in policy["scopes"]:
            for pattern in scope["manifestGlobs"]:
                files.update(p.relative_to(source_root).as_posix() for p in source_root.glob(pattern))
        for relative in sorted(files):
            destination = root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source_root / relative, destination)
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        env = dict(os.environ)
        for name in tuple(env):
            if name.startswith(("GENESIS_CARGO_CACHE_", "GENESIS_GENERATED_STATE_")) or name in policy["buildEnvironment"]:
                env.pop(name)
        env["GENESIS_CARGO_CACHE_ROOT"] = str(root / ".genesis/build/cargo-cache/v1")
        env["GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON"] = json.dumps({
            "release": "1.90.0", "commit-hash": "0" * 40, "host": "test-host-triple",
        })
        result = cache.resolve(root, "root-host", env)
        payload = cache.pretty_bytes(result["metadata"])
        target = Path(result["target_dir"])
        relative = target.relative_to(root).as_posix()
        metadata = target / result["metadata_file"]

        def admit(metadata_only=True, **kwargs):
            return state.admit(root, "cargo-cache", result["metadata"]["cacheKeySha256"], relative,
                               "cargo-host", pid=os.getpid(), metadata_payload=payload if metadata_only else None,
                               **kwargs)

        with patch.dict(os.environ, env, clear=True), patch.object(state, "_free_bytes", return_value=1 << 30), contextlib.redirect_stdout(io.StringIO()):
            require(cache.main(["--root", str(root), "--scope", "root-host", "--format", "json"]) == 0,
                    "metadata-only public CLI required a build reservation")
        require(metadata.read_bytes() == payload and state.status(root)["activeLeases"] == 0,
                "metadata output or transient release is wrong")
        controls.append("public-json-metadata-under-build-floor")

        first = admit(free_bytes_override=1 << 30)
        second = admit(free_bytes_override=1 << 30)
        require(second["pendingGrowthBytes"] == 2 * first["pendingGrowthBytes"], "concurrent metadata temporaries shared one budget")
        require(second["pendingGrowthBytes"] < 1 << 30, "metadata acquired a build-class reservation")
        try:
            with state.cleanup_guard(root, [".genesis/build"]):
                raise AssertionError("cleanup entered while metadata was live")
        except state.GeneratedStateError as exc:
            require("active generated-state lease" in str(exc), "wrong live-metadata cleanup diagnostic")
        controls.append("metadata-growth-additive-and-cleanup-protected")
        state.release(root, first["leaseToken"])
        require(state.validate_lease(root, second["leaseToken"], relative)["valid"], "one release invalidated another metadata writer")
        rejected(lambda: admit(False, free_bytes_override=1 << 30), "low-disk admission denied")
        require(state.status(root)["activeLeases"] == 1, "failed build leaked or discarded a lease")
        build = admit(False, free_bytes_override=32 << 30)
        require(build["pendingGrowthBytes"] > 4 << 30, "live build lost its full reservation")
        rejected(lambda: admit(free_bytes_override=1 << 30), "low-disk admission denied")
        state.release(root, second["leaseToken"])
        require(state.status(root)["accountingBytes"] >= 4 << 30, "metadata release discarded a live build reservation")
        state.release(root, build["leaseToken"])
        controls.append("mixed-build-metadata-preserves-build-growth-and-floor")

        with ThreadPoolExecutor(max_workers=4) as writers:
            receipts = list(writers.map(lambda _: admit(free_bytes_override=1 << 30), range(4)))
        require(len({item["leaseToken"] for item in receipts}) == 4,
                "concurrent writers shared a lease identity")
        require(state.status(root)["activeLeases"] == 4
                and max(item["pendingGrowthBytes"] for item in receipts) == 4 * first["pendingGrowthBytes"],
                "serialized admission lost a concurrent growth reservation")
        for item in receipts:
            state.release(root, item["leaseToken"])
        controls.append("parallel-writers-serialize-additive-reservations")

        rejected(lambda: admit(free_bytes_override=1 << 30, environ={"GENESIS_GENERATED_STATE_MIN_FREE_BYTES": str(2 << 30)}),
                 "low-disk admission denied")
        controls.append("explicit-caller-space-requirement-preserved")
        rejected(lambda: admit(free_bytes_override=1), "low-disk admission denied")
        require(metadata.read_bytes() == payload and state.status(root)["activeLeases"] == 0,
                "exhausted metadata admission wrote state or leaked a lease")
        controls.append("metadata-own-growth-and-journal-remain-required")

        first = admit(free_bytes_override=1 << 30)
        quota = first["accountingBytes"] + first["pendingGrowthBytes"]
        limits = {"GENESIS_GENERATED_STATE_HARD_BYTES": str(quota), "GENESIS_GENERATED_STATE_SOFT_BYTES": str(quota)}
        second = admit(free_bytes_override=1 << 30, environ=limits)
        rejected(lambda: admit(free_bytes_override=1 << 30, environ=limits), "hard quota admission denied")
        require(state.status(root)["activeLeases"] == 2, "quota denial changed live metadata ownership")
        state.release(root, first["leaseToken"])
        state.release(root, second["leaseToken"])
        controls.append("metadata-writers-cannot-double-spend-hard-quota")

        build = admit(False, free_bytes_override=32 << 30)
        registry_path = root / ".genesis/build/.generated-state-v0.1/registry.json"
        legacy = json.loads(registry_path.read_text())
        legacy.update(kind="genesis/generated-state-registry-v0.1", version="0.1")
        for lease in legacy["leases"]:
            lease.pop("operation"); lease.pop("growthBytes")
        registry_path.write_bytes(state.pretty_bytes(legacy))
        before = registry_path.read_bytes()
        require(state.status(root)["accountingBytes"] >= 4 << 30 and registry_path.read_bytes() == before,
                "legacy observation lowered build growth or persisted migration")
        rejected(lambda: admit(free_bytes_override=1 << 30), "low-disk admission denied")
        state.release(root, build["leaseToken"])
        require(json.loads(registry_path.read_text())["version"] == "0.2", "mutation did not migrate validated legacy leases")
        controls.append("legacy-read-preserved-and-atomic-mutation-migrates")

        active = admit(free_bytes_override=1 << 30)
        registry = json.loads(registry_path.read_text())
        loaded, _, identity = state.load_policy(root)
        for field, value in (("operation", "unknown"), ("growthBytes", 0), ("growthBytes", True)):
            candidate = copy.deepcopy(registry); candidate["leases"][0][field] = value
            rejected(lambda: state._validate_registry(candidate, loaded, identity),
                     "unknown generated-state lease operation" if field == "operation" else "growth")
        candidate = copy.deepcopy(registry); candidate["entries"][0]["reservationBytes"] = 0
        rejected(lambda: state._validate_registry(candidate, loaded, identity), "producer policy")
        state.release(root, active["leaseToken"])
        controls.append("closed-operation-decoder-and-policy-bound-entry-reservations")

        original_admit = state.admit
        frozen = cache.prepare_materialization(result)
        metadata.unlink()
        def mutate_after_admission(*args, **kwargs):
            receipt = original_admit(*args, **kwargs)
            result["metadata"]["policySha256"] = "0" * 64
            return receipt
        with patch.object(state, "admit", side_effect=mutate_after_admission), patch.object(state, "_free_bytes", return_value=1 << 30):
            cache.materialize_admitted(root, result)
        require(metadata.read_bytes() == frozen.payload, "writer recomputed mutable bytes after measuring admission")
        result["metadata"] = json.loads(frozen.payload)
        controls.append("admission-bound-to-frozen-written-payload")

        metadata.write_bytes(payload + b"x" * (2 << 20))
        actual_fdopen = cache.os.fdopen
        offsets = []
        class ReadObserver:
            def __init__(self, file): self.file = file
            def __enter__(self): self.file.__enter__(); return self
            def read(self, amount):
                value = self.file.read(amount)
                offsets.append(os.lseek(self.file.fileno(), 0, os.SEEK_CUR))
                return value
            def __exit__(self, *args): return self.file.__exit__(*args)
        with patch.object(cache.os, "fdopen", side_effect=lambda *args, **kwargs: ReadObserver(actual_fdopen(*args, **kwargs))):
            rejected(lambda: cache.materialize(result), "metadata mismatch")
        require(offsets == [len(payload) + 1], "corrupt metadata consumed unbounded bytes or buffered ahead")
        metadata.write_bytes(payload)
        controls.append("metadata-mismatch-uses-one-byte-unbuffered-probe")

        if not hasattr(os, "mkfifo"):
            raise AssertionError("metadata admission controls require the declared POSIX guard profile")
        metadata.unlink(); os.mkfifo(metadata)
        inode = metadata.stat().st_ino
        run = subprocess.run([sys.executable, str(root / "scripts/lib/cargo_cache.py"), "--root", str(root), "--scope", "root-host", "--format", "json"],
                             env=env, text=True, capture_output=True, timeout=5)
        require(run.returncode == 2 and "not a regular file" in run.stderr and metadata.stat().st_ino == inode,
                "FIFO metadata blocked or was mutated")
        require(state.status(root)["activeLeases"] == 0, "FIFO rejection leaked metadata lease")
        metadata.unlink(); metadata.write_bytes(payload)
        controls.append("public-fifo-rejection-releases-lease")

        oversized = copy.deepcopy(result); oversized["metadata"]["padding"] = "x" * state.MAX_JSON_BYTES
        rejected(lambda: cache.materialize_admitted(root, oversized), "bounded materialization payload")
        require(metadata.read_bytes() == payload and state.status(root)["activeLeases"] == 0,
                "oversized preparation wrote or leased state")
        controls.append("oversized-preparation-denied-before-materialization")

        # Discovery precedes the materializer when an entry is not registered.
        saved_registry = registry_path.read_bytes()
        registry_path.write_bytes(state.pretty_bytes(state._new_registry(identity)))
        empty_registry = registry_path.read_bytes()
        metadata.unlink(); os.mkfifo(metadata)
        inode = metadata.stat().st_ino
        run = subprocess.run([sys.executable, str(root / "scripts/lib/cargo_cache.py"), "--root", str(root), "--scope", "root-host", "--format", "json"],
                             env=env, text=True, capture_output=True, timeout=5)
        require(run.returncode == 2 and "cannot be registered safely" in run.stderr
                and metadata.stat().st_ino == inode and registry_path.read_bytes() == empty_registry,
                "unregistered FIFO discovery blocked, mutated the entry or wrote a lease")
        metadata.unlink(); metadata.write_bytes(payload)
        registry_path.write_bytes(saved_registry)
        controls.append("public-unregistered-fifo-discovery-rejected-before-lease")

        document = root / "growing.json"
        document.write_bytes(b"{}")
        actual_fstat = state.os.fstat
        def grow_after_descriptor_observation(descriptor):
            observed = actual_fstat(descriptor)
            document.write_bytes(b" " * (state.MAX_JSON_BYTES + 1) + b"{}")
            return observed
        offsets = []
        with patch.object(state.os, "fstat", side_effect=grow_after_descriptor_observation), patch.object(state.os, "fdopen", side_effect=lambda *args, **kwargs: ReadObserver(actual_fdopen(*args, **kwargs))):
            rejected(lambda: state.load_json(document), "JSON input exceeds")
        require(offsets and offsets[-1] == state.MAX_JSON_BYTES + 1,
                "growing JSON consumed beyond the bounded one-byte probe")
        controls.append("discovery-json-growth-bounded-after-descriptor-observation")

        document.write_bytes(b" " * (state.MAX_JSON_BYTES - 2) + b"{}")
        require(state.load_json(document) == {}, "exact-limit JSON rejected")
        document.write_bytes(b"\xff")
        rejected(lambda: state.load_json(document), "JSON input is not UTF-8")
        document.write_bytes(b'{"x":1,"x":2}')
        rejected(lambda: state.load_json(document), "duplicate JSON key")
        document.write_bytes(b"{}")
        policy_link = root / "regular-json-link"
        policy_link.symlink_to(document.name)
        require(state.load_json(policy_link) == {}, "explicit regular policy-link compatibility changed")
        controls.append("descriptor-json-limit-encoding-and-duplicate-contract")

        saved_path = registry_path.with_name("saved-registry.json")
        registry_path.rename(saved_path)
        registry_path.symlink_to("absent-registry-target.json")
        registry_inode = registry_path.lstat().st_ino
        try:
            run = subprocess.run([sys.executable, str(root / "scripts/lib/generated_state.py"), "--root", str(root), "status", "--format", "json"],
                                 env=env, text=True, capture_output=True, timeout=5)
            require(run.returncode == 2 and "Traceback" not in run.stderr
                    and registry_path.is_symlink() and registry_path.lstat().st_ino == registry_inode
                    and not registry_path.with_name("absent-registry-target.json").exists(),
                    "dangling registry link was followed or treated as absent writable state")
        finally:
            registry_path.unlink(); saved_path.rename(registry_path)
        controls.append("public-dangling-registry-entry-rejected-without-mutation")

        registry_path.write_bytes(empty_registry)
        metadata.unlink(); metadata.symlink_to(document)
        link_inode = metadata.lstat().st_ino
        run = subprocess.run([sys.executable, str(root / "scripts/lib/cargo_cache.py"), "--root", str(root), "--scope", "root-host", "--format", "json"],
                             env=env, text=True, capture_output=True, timeout=5)
        require(run.returncode == 2 and "Traceback" not in run.stderr
                and metadata.is_symlink() and metadata.lstat().st_ino == link_inode
                and document.read_bytes() == b"{}" and registry_path.read_bytes() == empty_registry,
                "metadata discovery followed or mutated a final symlink")
        metadata.unlink(); metadata.write_bytes(payload); registry_path.write_bytes(saved_registry)
        controls.append("public-unregistered-metadata-link-rejected-without-mutation")

        private_policy_path = root / state.POLICY_REL
        policy_bytes = private_policy_path.read_bytes()
        actual_loads = state.json.loads
        def replace_after_policy_decode(*args, **kwargs):
            decoded = actual_loads(*args, **kwargs)
            private_policy_path.write_bytes(b"{}")
            return decoded
        try:
            with patch.object(state.json, "loads", side_effect=replace_after_policy_decode):
                parsed, _, parsed_identity = state.load_policy(root)
            require(parsed["kind"] == "genesis/generated-state-policy-v0.1"
                    and parsed_identity == state.digest_bytes(policy_bytes),
                    "policy identity hashes bytes other than the opened/parsed payload")
        finally:
            private_policy_path.write_bytes(policy_bytes)
        controls.append("policy-identity-bound-to-one-frozen-descriptor-read")

        document.write_bytes(b"[" * 100000 + b"0" + b"]" * 100000)
        rejected(lambda: state.load_json(document), "decoder nesting limit")
        previous_integer_limit = sys.get_int_max_str_digits()
        try:
            sys.set_int_max_str_digits(4300)
            document.write_bytes(b"9" * 5000)
            rejected(lambda: state.load_json(document), "decoder value domain")
        finally:
            sys.set_int_max_str_digits(previous_integer_limit)
        document.write_bytes(b"{}")
        opened = []
        actual_open = state.os.open
        def observe_open(*args, **kwargs):
            descriptor = actual_open(*args, **kwargs)
            opened.append(descriptor)
            return descriptor
        with patch.object(state.os, "open", side_effect=observe_open), patch.object(state, "bytearray", side_effect=MemoryError, create=True):
            rejected(lambda: state.load_json(document), "bounded JSON input allocation failed")
        require(len(opened) == 1, "allocation fault did not exercise one opened descriptor")
        try:
            os.fstat(opened[0])
        except OSError as exc:
            require(exc.errno == errno.EBADF, "allocation failure closed the wrong descriptor")
        else:
            raise AssertionError("JSON allocation rejection leaked its descriptor")
        controls.append("decoder-domain-and-allocation-errors-close-file-descriptors")

        active = admit(free_bytes_override=1 << 30)
        registry = state.load_json(registry_path)
        for field, value in (("owner", []), ("sizeClass", {}), ("contentKey", []), ("retentionClass", [])):
            candidate = copy.deepcopy(registry); candidate["entries"][0][field] = value
            rejected(lambda: state._validate_registry(candidate, loaded, identity), "generated-state entry")
        candidate = copy.deepcopy(registry); candidate["leases"][0]["entryId"] = []
        rejected(lambda: state._validate_registry(candidate, loaded, identity), "unknown entry")
        candidate = copy.deepcopy(registry)
        candidate["transaction"] = {"id": "a" * 64, "entryId": [], "sourcePath": relative,
                                    "quarantinePath": ".genesis/cleanup-quarantine/request", "phase": "planned"}
        rejected(lambda: state._validate_registry(candidate, loaded, identity), "unknown entry")
        state.release(root, active["leaseToken"])
        before_registry = registry_path.read_bytes()
        identity_calls = []
        rejected(lambda: state.admit(root, "cargo-cache", result["metadata"]["cacheKeySha256"], relative,
                                    "cargo-host", pid=0, identity_fn=lambda pid: identity_calls.append(pid) or "a" * 64,
                                    free_bytes_override=1 << 30), "lease.pid")
        require(not identity_calls and registry_path.read_bytes() == before_registry,
                "explicit invalid PID acquired parent authority or mutated registry state")
        require(all(state.process_identity(pid) is None for pid in (True, 0, -1, 1.5, "1", 10**100)),
                "invalid or unrepresentable PID escaped the process identity boundary")
        controls.append("closed-registry-references-and-explicit-process-id-admission")

        registry_now = registry_path.read_bytes()
        registry_path.write_bytes(empty_registry)
        original_prepare = cache.prepare_materialization
        original_metadata = copy.deepcopy(result["metadata"])
        def mutate_identity_and_class_after_freeze(value):
            prepared = original_prepare(value)
            value["metadata"]["cacheKeySha256"] = "b" * 64
            value["metadata"]["cacheKey"]["buildEnvironment"].update(
                CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0",
            )
            return prepared
        try:
            with patch.object(cache, "prepare_materialization", side_effect=mutate_identity_and_class_after_freeze), patch.object(state, "_free_bytes", return_value=32 << 30):
                cache.materialize_admitted(root, result, lease_pid=os.getpid())
            bound = state.load_json(registry_path)
            require(bound["entries"][0]["contentKey"] == original_metadata["cacheKeySha256"]
                    and bound["entries"][0]["sizeClass"] == "cargo-host"
                    and result["generated_state"]["pendingGrowthBytes"] > 4 << 30
                    and metadata.read_bytes() == payload,
                    "mutable result changed identity/class after the frozen plan")
            state.release(root, result["generated_state"]["leaseToken"])
        finally:
            result.pop("generated_state", None)
            result["metadata"] = original_metadata
            registry_path.write_bytes(registry_now)
        controls.append("frozen-metadata-binds-admission-identity-and-build-class")

        registry_path.write_bytes(empty_registry)
        malformed = copy.deepcopy(original_metadata)
        malformed["cacheKey"]["buildEnvironment"] = []
        metadata.write_bytes(cache.pretty_bytes(malformed))
        metadata_inode = metadata.stat().st_ino
        run = subprocess.run([sys.executable, str(root / "scripts/lib/cargo_cache.py"), "--root", str(root), "--scope", "root-host", "--format", "json"],
                             env=env, text=True, capture_output=True, timeout=5)
        require(run.returncode == 2 and "build environment is invalid" in run.stderr
                and "Traceback" not in run.stderr and metadata.stat().st_ino == metadata_inode
                and metadata.read_bytes() == cache.pretty_bytes(malformed)
                and registry_path.read_bytes() == empty_registry,
                "malformed discovery environment escaped or mutated state")
        metadata.write_bytes(payload); registry_path.write_bytes(registry_now)
        controls.append("public-discovery-build-environment-type-admitted")
        require((root / state.POLICY_REL).read_bytes() == (source_root / state.POLICY_REL).read_bytes(),
                "production policy was changed for operation controls")
    require(len(controls) == 23, "control inventory drift")
    print("cargo-metadata-admission: " + json.dumps({"controls": controls}, sort_keys=True))
    return len(controls)


if __name__ == "__main__":
    metadata_self_test(Path(__file__).resolve().parents[2])
