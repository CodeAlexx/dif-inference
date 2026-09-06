await sleep(2500);
{ const inp = document.querySelector("#gen-model-search"); if (!/klein-base-9b/.test(inp.value)) { inp.focus(); inp.click(); inp.dispatchEvent(new Event("focus")); inp.value = "klein-base-9b"; inp.dispatchEvent(new Event("input",{bubbles:true})); await sleep(800); const it = [...document.querySelectorAll(".gen-model-dropdown-item")].find(e => /flux-2-klein-base-9b/.test(e.textContent)); if (it) { it.click(); await sleep(800); } } log("model", inp.value, "|", document.querySelector("#gen-runtime-label").textContent); }
const set = (id, v) => { const e = document.querySelector(id); e.value = v; e.dispatchEvent(new Event('input',{bubbles:true})); e.dispatchEvent(new Event('change',{bubbles:true})); };
set('#gen-prompt', 'a red kite over a green hill'); set('#gen-steps', '50'); set('#gen-seed', '5150'); set('#gen-toolbar-batch-input', '3');
const jobsBefore = await fetch('/v1/jobs').then(r=>r.json());
document.querySelector('#gen-btn').click();
await sleep(6000);
let jobs = await fetch('/v1/jobs').then(r=>r.json()); let mine = jobs.filter(x => !jobsBefore.some(y => y.id === x.id));
log('after submit', mine.map(j => j.id + ':' + j.state));
document.querySelector('[data-tab="queue"], #tab-queue, button[data-tab="queue"]')?.click(); await sleep(1500);
await shot('p2-08-queue-before');
const removeBtns = [...document.querySelectorAll('.queue-pending-remove')]; log('pending remove buttons', removeBtns.map(b => b.dataset.id));
if (removeBtns[0]) removeBtns[0].click();
await sleep(2500);
jobs = await fetch('/v1/jobs').then(r=>r.json()); mine = jobs.filter(x => !jobsBefore.some(y => y.id === x.id));
log('after remove', mine.map(j => j.id + ':' + j.state));
const cur = document.querySelector('#queue-cancel-current'); log('cancel-current button', !!cur); if (cur) cur.click();
await sleep(4000);
jobs = await fetch('/v1/jobs').then(r=>r.json()); mine = jobs.filter(x => !jobsBefore.some(y => y.id === x.id));
log('after cancel current', mine.map(j => j.id + ':' + j.state));
await shot('p2-09-queue-after');
// cancel whatever is left so the GPU is free
for (const j of mine) if (!['done','failed','cancelled'].includes(j.state)) await fetch('/v1/cancel/' + j.id, {method:'POST'});
await sleep(3000);
jobs = await fetch('/v1/jobs').then(r=>r.json()); mine = jobs.filter(x => !jobsBefore.some(y => y.id === x.id));
log('final', mine.map(j => j.id + ':' + j.state));
mine.map(j => j.id + ':' + j.state);
