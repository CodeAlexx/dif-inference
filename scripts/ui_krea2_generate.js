await sleep(2500);
{ const inp = document.querySelector("#gen-model-search"); if (!/krea2-turbo/.test(inp.value)) { inp.focus(); inp.click(); inp.dispatchEvent(new Event("focus")); inp.value = "krea2-turbo"; inp.dispatchEvent(new Event("input",{bubbles:true})); await sleep(800); const it = [...document.querySelectorAll(".gen-model-dropdown-item")].find(e => /krea2-turbo/.test(e.textContent)); if (it) { it.click(); await sleep(800); } } log("model", inp.value, "|", document.querySelector("#gen-runtime-label").textContent); }
const set = (id, v) => { const e = document.querySelector(id); e.value = v; e.dispatchEvent(new Event('input',{bubbles:true})); e.dispatchEvent(new Event('change',{bubbles:true})); };
set('#gen-prompt', 'a ceramic bowl of ripe figs on a marble counter, soft window light, (film grain:1.1)');
set('#gen-steps', '8'); set('#gen-seed', '31');
const jobsBefore = await fetch('/v1/jobs').then(r=>r.json());
document.querySelector('#gen-btn').click();
let j = null;
for (let i = 0; i < 90; i++) { await sleep(1000); const jobs = await fetch('/v1/jobs').then(r=>r.json()); j = jobs.find(x => !jobsBefore.some(y => y.id === x.id)); if (j && ['done','failed','cancelled'].includes(j.state)) break; }
const note = document.querySelector('#gen-prompt-syntax-note');
log('note', note ? note.textContent : 'no note el', 'display', note && note.style.display);
const full = await fetch('/v1/job/' + j.id).then(r=>r.json());
const params = full.metadata && full.metadata.params || {};
log('job', j.id, j.state, 'prompt=', params.prompt, '| prompt_raw=', params.prompt_raw);
await shot('p4-03-krea2-ui');
({ state: j.state, prompt: params.prompt, prompt_raw: params.prompt_raw });
