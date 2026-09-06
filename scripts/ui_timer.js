await sleep(2500);
const timerEl = () => document.querySelector('#gen-run-timer');
log('timer initial:', timerEl() ? timerEl().textContent : 'MISSING');
{ const inp = document.querySelector("#gen-model-search"); if (!/krea2-turbo/.test(inp.value)) { inp.focus(); inp.click(); inp.dispatchEvent(new Event("focus")); inp.value = "krea2-turbo"; inp.dispatchEvent(new Event("input",{bubbles:true})); await sleep(800); const it = [...document.querySelectorAll(".gen-model-dropdown-item")].find(e => /krea2-turbo/.test(e.textContent)); if (it) { it.click(); await sleep(800); } } log("model", inp.value); }
const set = (id, v) => { const e = document.querySelector(id); e.value = v; e.dispatchEvent(new Event('input',{bubbles:true})); e.dispatchEvent(new Event('change',{bubbles:true})); };
set('#gen-prompt', 'a red bicycle leaning on a stone wall, morning light');
set('#gen-steps', '8'); set('#gen-seed', '44');
await shot('timer-01-idle');
const t0 = Date.now();
document.querySelector('#gen-btn').click();
await sleep(3000);
log('timer running:', timerEl().textContent, 'class', timerEl().className);
await shot('timer-02-running');
let text = '';
for (let i = 0; i < 120; i++) { await sleep(1000); const el = timerEl(); text = el.textContent; if (!el.classList.contains('running') && i > 3) break; }
const wall = (Date.now() - t0) / 1000;
await sleep(500); await shot('timer-03-done');
log('timer final:', text, '| script wall', wall.toFixed(1), 's');
({ final: text, script_wall_s: wall });
