await sleep(2500);
{ const inp = document.querySelector("#gen-model-search"); if (!/klein-base-9b/.test(inp.value)) { inp.focus(); inp.click(); inp.dispatchEvent(new Event("focus")); inp.value = "klein-base-9b"; inp.dispatchEvent(new Event("input",{bubbles:true})); await sleep(800); const it = [...document.querySelectorAll(".gen-model-dropdown-item")].find(e => /flux-2-klein-base-9b/.test(e.textContent)); if (it) { it.click(); await sleep(800); } } log("model", inp.value, "|", document.querySelector("#gen-runtime-label").textContent); }
const set = (id, v) => { const e = document.querySelector(id); e.value = v; e.dispatchEvent(new Event('input',{bubbles:true})); e.dispatchEvent(new Event('change',{bubbles:true})); };
document.querySelector('#gen-grid-header').click(); await sleep(300);
set('#gen-prompt', 'a ceramic teapot on a linen cloth, studio light');
set('#gen-steps', '6'); set('#gen-seed', '7');
set('#gen-grid-x-axis', 'seed'); set('#gen-grid-x-values', '1,2');
set('#gen-grid-y-axis', 'cfg'); set('#gen-grid-y-values', '2,4');
await shot('p2-06-grid-form');
document.querySelector('#gen-grid-run').click();
let status = '';
for (let i = 0; i < 400; i++) { await sleep(1000); status = document.querySelector('#gen-grid-status').textContent; if (/done|failed/.test(status)) break; if (i % 30 === 0) log('grid status', status); }
log('final', status);
await sleep(1500); await shot('p2-07-grid-result');
const img = document.querySelector('#gen-preview-img'); log('preview', img.src, img.naturalWidth + 'x' + img.naturalHeight);
({ status, preview: img.src, w: img.naturalWidth, h: img.naturalHeight });
