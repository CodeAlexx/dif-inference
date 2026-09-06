#!/usr/bin/env python3
"""Submit one /v1/generate request and poll /v1/job/<id> to a terminal state.
usage: gen.py PROMPT [--steps N] [--seed N] [--cfg F] [--w N] [--h N] [--model NAME] [--port N] [--cancel-at-step N] [--extra JSON]
Prints one line per state change and the final job record as JSON."""
import argparse, json, sys, time, urllib.request
ap = argparse.ArgumentParser()
ap.add_argument("prompt"); ap.add_argument("--steps", type=int, default=50); ap.add_argument("--seed", type=int, default=7)
ap.add_argument("--cfg", type=float, default=4.0); ap.add_argument("--w", type=int, default=1024); ap.add_argument("--h", type=int, default=1024)
ap.add_argument("--model", default="flux-2-klein-base-9b"); ap.add_argument("--port", type=int, default=7811)
ap.add_argument("--cancel-at-step", type=int, default=-1); ap.add_argument("--extra", default="{}")
a = ap.parse_args()
base = f"http://127.0.0.1:{a.port}"
def call(path, body=None):
    req = urllib.request.Request(base + path, data=json.dumps(body).encode() if body is not None else None,
                                 headers={"content-type": "application/json"}, method="POST" if body is not None else "GET")
    try:
        with urllib.request.urlopen(req, timeout=30) as r: return r.status, json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e: return e.code, json.loads(e.read() or b"null")
body = {"model": a.model, "prompt": a.prompt, "width": a.w, "height": a.h, "steps": a.steps, "cfg": a.cfg, "seed": a.seed,
        "sampler": "euler", "scheduler": "simple"}
body.update(json.loads(a.extra))
t0 = time.time()
st, pre = call("/v1/preflight", body); print("preflight", st, json.dumps(pre)[:300])
st, gen = call("/v1/generate", body); print("generate", st, gen)
if st != 200: sys.exit(1)
jid = gen["job_id"]; last = None; cancelled = False
while True:
    st, j = call(f"/v1/job/{jid}")
    key = (j.get("state"), j.get("step"), j.get("progress"))
    if key != last:
        print(f"{time.time()-t0:7.1f}s state={j.get('state')} step={j.get('step')}/{j.get('total')} progress={j.get('progress')} err={j.get('error')!r}", flush=True)
        last = key
    if a.cancel_at_step >= 0 and not cancelled and j.get("state") == "running" and (j.get("step") or 0) >= a.cancel_at_step:
        st, c = call(f"/v1/cancel/{jid}", {}); print(f"{time.time()-t0:7.1f}s cancel -> {st} {c}", flush=True); cancelled = True
    if j.get("state") in ("done", "failed", "cancelled", "interrupted"):
        print("FINAL", json.dumps(j)); print(f"wall {time.time()-t0:.1f}s"); break
    time.sleep(2)
