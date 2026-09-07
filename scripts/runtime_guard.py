#!/usr/bin/env python3
"""Host/process control only. No model code, tensor operations or GPU imports.

A per-child MemoryMax does not protect an ancestor monitored by systemd-oomd.
Watch both host reserve and ancestor/child PSI, cancelling only our exact unit.
Never change oomd settings or kill another desktop/client process.
"""
import argparse
from dataclasses import dataclass
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import time


@dataclass(frozen=True)
class Policy:
    sample_seconds: float
    pressure_window_seconds: float
    full_pressure_percent: float
    some_pressure_percent: float
    admission_pressure_percent: float
    child_soft_fraction: float
    startup_timeout_seconds: float
    maximum_runtime_seconds: float

    @classmethod
    def read(cls, path):
        raw = json.loads(Path(path).read_text())["runtime"]["memory_guard"]
        fields = {}
        for name in cls.__dataclass_fields__:
            v = raw[name]
            if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v) or v <= 0:
                raise ValueError(f"Invalid memory guard {name}")
            fields[name] = v
        result = cls(**fields)
        if not (0.1 <= result.sample_seconds <= 1 and
                result.sample_seconds <= result.pressure_window_seconds <= 5 and
                result.full_pressure_percent < result.some_pressure_percent <= 25 and
                result.admission_pressure_percent <= result.full_pressure_percent and
                0.5 <= result.child_soft_fraction <= 0.95 and
                result.startup_timeout_seconds <= 30):
            raise ValueError("Memory guard thresholds outside safe bounds")
        return result, raw


def key_values(path):
    return {row.split()[0].rstrip(":"): int(row.split()[1])
            for row in Path(path).read_text().splitlines()}


def psi(path):
    result = {}
    for line in Path(path).read_text().splitlines():
        kind, *fields = line.split()
        values = dict(v.split("=") for v in fields)
        result[kind] = {"total": int(values["total"]), "avg10": float(values["avg10"])}
    if set(result) != {"some", "full"}:
        raise ValueError(f"Invalid memory pressure counters: {path}")
    return result


class Reader:
    def __init__(self, parent, child=None):
        self.parent, self.child = Path(parent), Path(child) if child else None

    def populated(self):
        try:
            return bool(key_values(self.child / "cgroup.events")["populated"])
        except FileNotFoundError:
            return False

    def snapshot(self):
        host = key_values("/proc/meminfo")
        stats = key_values(self.parent / "memory.stat")
        pressure = {"host": psi("/proc/pressure/memory")}
        # Include user@, user-UID.slice and user.slice: any monitored ancestor
        # can be under pressure even though this child's cap has not been hit.
        for path in [self.parent, *self.parent.parents]:
            if path == Path("/sys/fs/cgroup"):
                break
            if path.is_relative_to("/sys/fs/cgroup"):
                pressure[str(path)] = psi(path / "memory.pressure")
        result = {"time": time.monotonic(), "available": host["MemAvailable"] * 1024,
                  "total": host["MemTotal"] * 1024,
                  "session_nonreclaimable": sum(stats[k] for k in ("anon", "shmem", "kernel")),
                  "session_current": int((self.parent / "memory.current").read_text()),
                  "pressure": pressure, "child_current": None, "child_peak": 0, "events": {}}
        if self.child and self.child.exists():
            result["child_current"] = int((self.child / "memory.current").read_text())
            result["child_peak"] = int((self.child / "memory.peak").read_text())
            result["events"] = key_values(self.child / "memory.events")
            child_stats = key_values(self.child / "memory.stat")
            result["child_memory"] = {key: child_stats[key] for key in
                ("anon", "file", "shmem", "kernel", "file_mapped", "file_dirty", "file_writeback",
                 "pgfault", "pgmajfault", "pgscan", "pgsteal")}
            pressure["child"] = psi(self.child / "memory.pressure")
        return result


def admission(sample, policy, maximum, reserve):
    if sample["available"] < maximum + reserve:
        return "insufficient host headroom for child ceiling plus desktop reserve"
    if sample["session_nonreclaimable"] + maximum > sample["total"] - reserve:
        return "session plus child ceiling violates desktop reserve"
    if any(v["avg10"] >= policy.admission_pressure_percent
           for source in sample["pressure"].values() for v in source.values()):
        return "memory pressure already elevated before admission"
    return None


class Guard:
    def __init__(self, policy, maximum, reserve):
        self.policy, self.maximum, self.reserve = policy, maximum, reserve
        self.history = []

    def observe(self, sample):
        if sample["available"] < self.reserve:
            return "host desktop reserve exhausted"
        if sample["session_nonreclaimable"] > sample["total"] - self.reserve:
            return "session nonreclaimable memory exceeds reserve budget"
        child = sample["child_current"]
        if child is not None and child >= self.maximum * self.policy.child_soft_fraction:
            return "child soft ceiling reached before MemoryMax reclaim"
        if any(sample["events"].get(k, 0) for k in ("high", "max", "oom", "oom_kill")):
            return "child reclaim/OOM counter incremented"
        for source, kinds in sample["pressure"].items():
            for kind, limit in [("full", self.policy.full_pressure_percent),
                                ("some", self.policy.some_pressure_percent)]:
                if kinds[kind]["avg10"] >= limit:
                    return f"{source} {kind} memory pressure avg10 reached {limit}%"
        self.history.append(sample)
        while len(self.history) > 1 and sample["time"] - self.history[1]["time"] >= self.policy.pressure_window_seconds:
            self.history.pop(0)
        first = self.history[0]
        elapsed = sample["time"] - first["time"]
        if elapsed >= self.policy.pressure_window_seconds:
            for source, kinds in sample["pressure"].items():
                if source not in first["pressure"]:
                    continue
                for kind, limit in [("full", self.policy.full_pressure_percent),
                                    ("some", self.policy.some_pressure_percent)]:
                    delta = kinds[kind]["total"] - first["pressure"][source][kind]["total"]
                    if delta < 0:
                        return "memory pressure counter reset during execution"
                    if delta / (elapsed * 10000) >= limit:
                        return f"{source} {kind} memory stalls reached {limit}% over {elapsed:.2f}s"
        return None


def alive(pid):
    try:
        # Exclude an unreaped systemd-run client; kill(pid,0) sees zombies.
        state = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0]
        return state != "Z"
    except FileNotFoundError:
        return False


def stop_unit(unit):
    if not re.fullmatch(r"serenity-runtime-memory-\d{8}-\d{6}-\d+", unit):
        raise ValueError("Refusing to kill an unrecognized runtime unit")
    subprocess.run(["systemctl", "--user", "kill", "--kill-whom=all", "--signal=SIGKILL", unit],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5, check=False)


def monitor(reader, policy, maximum, reserve, live, stop, emit, sleep=time.sleep,
            owner_live=lambda: True, populated=lambda: False):
    start = time.monotonic()
    guard = Guard(policy, maximum, reserve)
    peak = 0
    try:
        while live():
            try:
                sample = reader.snapshot()
            except FileNotFoundError:
                # Successful fast shutdown may remove the cgroup between the
                # live check and the read. Only accept this race when both the
                # launcher and all service processes are already gone.
                if not live() and not populated():
                    break
                raise
            peak = max(peak, sample["child_peak"])
            emit({"event": "sample", **sample})
            reason = guard.observe(sample)
            if not owner_live():
                reason = "owning wrapper exited; refusing an orphaned runtime"
            elapsed = time.monotonic() - start
            if sample["child_current"] is None and elapsed > policy.startup_timeout_seconds:
                reason = "runtime cgroup did not become observable"
            if elapsed > policy.maximum_runtime_seconds:
                reason = "runtime deadline reached"
            if reason:
                emit({"event": "cancel", "reason": reason, "peak_bytes": peak})
                stop()
                return 75
            sleep(policy.sample_seconds)
        if populated():
            emit({"event": "cancel", "reason": "launcher exited with runtime processes still alive",
                  "peak_bytes": peak})
            stop()
            return 75
        # The service may disappear between samples. This is the largest kernel
        # memory.peak observed, not a claim to capture a short final allocation.
        emit({"event": "finished", "observed_peak_bytes": peak})
        return 0
    except Exception as error:
        # Losing accounting/logging is a fail-closed condition, not permission
        # to leave a heavy child unmonitored.
        stop()
        print(f"runtime_guard: cancelled after monitoring error: {error}", file=sys.stderr)
        return 75


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["admit", "watch"])
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--parent", type=Path, required=True)
    parser.add_argument("--max-bytes", type=int, required=True)
    parser.add_argument("--reserve-bytes", type=int, required=True)
    parser.add_argument("--unit")
    parser.add_argument("--child", type=Path)
    parser.add_argument("--runner-pid", type=int)
    parser.add_argument("--owner-pid", type=int)
    args = parser.parse_args()
    if args.max_bytes <= 0 or args.reserve_bytes <= 0:
        parser.error("memory ceiling and reserve must be positive")
    policy, raw = Policy.read(args.policy)
    if args.command == "admit":
        reason = admission(Reader(args.parent).snapshot(), policy, args.max_bytes, args.reserve_bytes)
        if reason:
            print(f"runtime_guard: admission refused: {reason}", file=sys.stderr)
            return 75
        return 0
    if not args.unit or not args.child or not args.runner_pid or not args.owner_pid:
        parser.error("watch requires unit, child, runner-pid and owner-pid")
    if not re.fullmatch(r"serenity-runtime-memory-\d{8}-\d{6}-\d+", args.unit):
        parser.error("invalid unit name")
    log_dir = Path(raw["telemetry_dir"])
    if not log_dir.is_absolute():
        log_dir = args.policy.resolve().parent.parent / log_dir
    # If telemetry setup fails, the wrapper also cancels its child on nonzero.
    log_dir.mkdir(parents=True, exist_ok=True)
    path = log_dir / f"{args.unit}.jsonl"
    with path.open("x", buffering=1) as output:
        def emit(row):
            output.write(json.dumps(row, allow_nan=False) + "\n")
            if row["event"] != "sample":
                print(f"[runtime_guard] {json.dumps(row)} telemetry={path}", file=sys.stderr, flush=True)
        reader = Reader(args.parent, args.child)
        return monitor(reader, policy, args.max_bytes, args.reserve_bytes,
                       lambda: alive(args.runner_pid), lambda: stop_unit(args.unit), emit,
                       owner_live=lambda: alive(args.owner_pid), populated=reader.populated)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(f"runtime_guard: {error}", file=sys.stderr)
        sys.exit(75)
