await sleep(2500);
const inp = document.querySelector('#gen-model-search');
inp.focus(); inp.click(); inp.dispatchEvent(new Event('focus')); inp.value = 'MiniMax-H3'; inp.dispatchEvent(new Event('input',{bubbles:true})); await sleep(800);
const items = [...document.querySelectorAll('.gen-model-dropdown-item')].map(e => e.textContent.trim().slice(0,40));
log('h3 picker items', items);
const it = [...document.querySelectorAll('.gen-model-dropdown-item')].find(e => /MiniMax-H3 Base/.test(e.textContent));
if (it) { it.click(); await sleep(1200); }
log('model', inp.value, '| label', document.querySelector('#gen-runtime-label').textContent);
const vis = e => !!(e && e.offsetParent);
['gen-video-header','gen-image-header','gen-core-header'].forEach(id => { const h = document.getElementById(id); if (h && h.getAttribute('aria-expanded') !== 'true') h.click(); });
await sleep(400);
const selects = [...document.querySelectorAll('select')].filter(vis).map(s => ({id: s.id, value: s.value, opts: [...s.options].map(o => o.textContent.trim().slice(0,45)).slice(0,14)}));
log('visible selects', selects);
const inputs = [...document.querySelectorAll('input')].filter(vis).filter(e => /video|frame|fps|second|duration|steps|seed|width|height|h3/i.test(e.id)).map(e => ({id: e.id, type: e.type, value: e.value}));
log('video-ish inputs', inputs);
log('generate label', document.querySelector('#gen-btn').textContent.trim());
await shot('p3-01-h3-generate-tab');
return { model: inp.value, selects: selects.length };
