#!/usr/bin/env python3
"""Small safety tests; fault injection never allocates pressure-producing RAM."""
import datetime
import os
from pathlib import Path
import subprocess
import time
import unittest

from runtime_guard import Guard, Policy, admission, monitor, stop_unit

ROOT = Path(__file__).resolve().parents[1]
POLICY, _ = Policy.read(ROOT / "config/runtime.json")
GIB = 1024 ** 3


def sample(now=0):
    return {"time":now,"total":64*GIB,"available":50*GIB,
            "session_nonreclaimable":8*GIB,"session_current":35*GIB,
            "child_current":GIB,"child_peak":GIB,"events":{},
            "pressure":{"parent":{"some":{"total":0,"avg10":0},
                                   "full":{"total":0,"avg10":0}}}}


class PolicyTests(unittest.TestCase):
    def test_admission_reserves_maximum_and_checks_existing_pressure(self):
        s = sample()
        self.assertIsNone(admission(s, POLICY, 24*GIB, 16*GIB))
        s["available"] = 39*GIB
        self.assertIn("headroom", admission(s, POLICY, 24*GIB, 16*GIB))
        s = sample(); s["session_nonreclaimable"] = 25*GIB
        self.assertIn("session", admission(s, POLICY, 24*GIB, 16*GIB))
        s = sample(); s["pressure"]["parent"]["some"]["avg10"] = 6
        self.assertIn("pressure", admission(s, POLICY, 24*GIB, 16*GIB))

    def test_clean_checkpoint_cache_alone_is_not_an_oom_claim(self):
        s = sample(); s["session_current"] = 60*GIB
        self.assertIsNone(Guard(POLICY, 24*GIB, 16*GIB).observe(s))

    def test_live_headroom_and_child_soft_limit(self):
        for field, value in [("available",15*GIB), ("session_nonreclaimable",49*GIB),
                             ("child_current",22*GIB)]:
            s = sample(); s[field] = value
            self.assertIsNotNone(Guard(POLICY,24*GIB,16*GIB).observe(s))

    def test_recorded_incident_would_cancel_below_24g_cap(self):
        s = sample(); s["child_current"] = int(11.6*GIB)
        s["pressure"]["parent"]["full"]["avg10"] = 55.63
        self.assertIn("pressure", Guard(POLICY,24*GIB,16*GIB).observe(s))

    def test_recent_stalls_cancel_before_ten_second_average_catches_up(self):
        # 700000 over 2 s = 35%, above window_pressure_percent. The old fixture
        # used 250000 (12.5%), which a healthy streamed render exceeds on its
        # own: video-0063 measured a 14.93% worst 2 s window while avg10 stayed
        # at 1.15% and the child held 10.39 GB of a 44 GB cap. The property
        # under test -- a burst cancels before avg10 catches up -- is unchanged.
        guard = Guard(POLICY,24*GIB,16*GIB)
        self.assertIsNone(guard.observe(sample(0)))
        s = sample(2); s["pressure"]["parent"]["full"]["total"] = 700000
        self.assertIn("stalls", guard.observe(s))

    def test_streamed_read_burst_below_window_threshold_does_not_cancel(self):
        # Regression for four cancelled H3 renders (video-0051/0061/0062/0063):
        # ordinary streamed-checkpoint I/O produces ~15% over 2 s with no
        # scarcity at all. Recorded from telemetry 20260907-021450.
        guard = Guard(POLICY,24*GIB,16*GIB)
        self.assertIsNone(guard.observe(sample(0)))
        s = sample(2); s["pressure"]["parent"]["full"]["total"] = 298600
        self.assertIsNone(guard.observe(s))

    def test_short_stall_does_not_accumulate_forever(self):
        guard = Guard(POLICY,24*GIB,16*GIB)
        for second in range(20):
            s = sample(second); s["pressure"]["parent"]["full"]["total"] = 10000
            self.assertIsNone(guard.observe(s))

    def test_reclaim_and_missing_accounting_fail_closed(self):
        for name in ["high","max","oom","oom_kill"]:
            s = sample(); s["events"][name] = 1
            self.assertIn("counter", Guard(POLICY,24*GIB,16*GIB).observe(s))
        class Broken:
            def snapshot(self): raise OSError("fixture lost accounting")
        stopped = []
        self.assertEqual(monitor(Broken(),POLICY,24*GIB,16*GIB,lambda: True,
                                 lambda: stopped.append(True),lambda _: None),75)
        self.assertEqual(stopped,[True])

    def test_refuses_broad_or_foreign_kill_target(self):
        for unit in ["user@1000.service", "app.slice", "", "serenity-sam3"]:
            with self.assertRaises(ValueError): stop_unit(unit)

    def test_owner_death_and_orphaned_launcher_cancel(self):
        class Healthy:
            def snapshot(self): return sample()
        for live, owner, populated in [(True, False, True), (False, True, True)]:
            stopped, events = [], []
            status = monitor(Healthy(),POLICY,24*GIB,16*GIB,lambda: live,
                             lambda: stopped.append(True),events.append,
                             owner_live=lambda: owner,populated=lambda: populated)
            self.assertEqual(status,75)
            self.assertEqual(stopped,[True])
            self.assertEqual(events[-1]["event"],"cancel")

    def test_accounting_disappears_only_after_verified_shutdown(self):
        class Disappearing:
            def snapshot(self): raise FileNotFoundError('collected cgroup')
        checks = iter([True,False])
        events, stopped = [], []
        status = monitor(Disappearing(),POLICY,24*GIB,16*GIB,lambda: next(checks),
                         lambda: stopped.append(True),events.append)
        self.assertEqual(status,0)
        self.assertEqual(stopped,[])
        self.assertEqual(events[-1]['event'],'finished')


@unittest.skipUnless(os.environ.get("DIF_GUARD_LIVE_TEST") == "1", "explicit tiny service gate only")
class LiveTests(unittest.TestCase):
    def test_wrapper_failure_and_parent_death_do_not_leave_service(self):
        import json
        import re
        import signal
        env = dict(os.environ, MEM_MAX="128M",SWAP_MAX="0",DESKTOP_RESERVE="16G",MEM_HIGH="infinity")
        wrapper = str(ROOT / "scripts/mem_safe_runtime.sh")
        failed = subprocess.run([wrapper,"/usr/bin/false"],env=env,capture_output=True,text=True,timeout=10)
        self.assertEqual(failed.returncode,1,failed.stderr)
        for sig in [signal.SIGTERM, signal.SIGKILL]:
            job = subprocess.Popen([wrapper,"/usr/bin/sleep","30"],env=env,
                                   stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
            # Wait for the actual monitored service, not a guessed launch delay.
            unit = None
            try:
                deadline = time.monotonic()+8
                while time.monotonic() < deadline:
                    logs = list((ROOT / "output/runtime-guard").glob(f"*-{job.pid}.jsonl"))
                    if logs and logs[0].stat().st_size:
                        unit = logs[0].stem
                        rows = [json.loads(line) for line in logs[0].read_text().splitlines()]
                        if any(row.get("child_current") is not None for row in rows): break
                    time.sleep(.1)
                self.assertIsNotNone(unit)
                self.assertTrue(re.fullmatch(r"serenity-runtime-memory-\d{8}-\d{6}-\d+",unit))
                job.send_signal(sig)
                _, err = job.communicate(timeout=8)
                self.assertNotEqual(subprocess.run(["systemctl","--user","is-active","--quiet",unit]).returncode,0,err)
            finally:
                if job.poll() is None: job.kill()
                if unit: stop_unit(unit)
                job.communicate(timeout=8)

    def test_real_unit_cancel_leaves_sibling_alive(self):
        stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
        units = [f"serenity-runtime-memory-{stamp}-{os.getpid()+i}" for i in range(2)]
        try:
            for unit in units:
                subprocess.run(["systemd-run","--user","--quiet","--collect",f"--unit={unit}",
                                "--property=MemoryMax=64M","--property=MemorySwapMax=0",
                                "/usr/bin/sleep","15"],check=True)
            class InjectedReserveFailure:
                def snapshot(self):
                    s = sample(); s["available"] = 0; return s
            events = []
            status = monitor(InjectedReserveFailure(),POLICY,24*GIB,16*GIB,
                             lambda: True,lambda: stop_unit(units[0]),events.append)
            self.assertEqual(status,75)
            self.assertEqual(events[-1]["event"],"cancel")
            time.sleep(.2)
            self.assertNotEqual(subprocess.run(["systemctl","--user","is-active","--quiet",units[0]]).returncode,0)
            self.assertEqual(subprocess.run(["systemctl","--user","is-active","--quiet",units[1]]).returncode,0)
        finally:
            for unit in units: stop_unit(unit)

    def test_real_wrapper_finishes_and_refuses_concurrent_heavy_job(self):
        env = dict(os.environ, MEM_MAX="128M",SWAP_MAX="0",DESKTOP_RESERVE="16G",MEM_HIGH="infinity")
        wrapper = str(ROOT / "scripts/mem_safe_runtime.sh")
        first = subprocess.Popen([wrapper,"/usr/bin/sleep","3"],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        try:
            time.sleep(.6)
            second = subprocess.run([wrapper,"/usr/bin/true"],env=env,capture_output=True,text=True)
            self.assertEqual(second.returncode,75,second.stderr)
            self.assertIn("another guarded heavy job",second.stderr)
            _, err = first.communicate(timeout=10)
            self.assertEqual(first.returncode,0,err)
            self.assertIn('"event": "finished"',err)
        finally:
            if first.poll() is None:
                first.terminate(); first.communicate(timeout=10)


if __name__ == "__main__": unittest.main(verbosity=2)
