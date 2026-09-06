await sleep(2500);
const inp = document.querySelector('#gen-model-search');
log('current model field', inp.value);
inp.focus(); inp.click(); inp.dispatchEvent(new Event('focus'));
await sleep(600);
const cands = [...document.querySelectorAll('li,div,button,option')].filter(e => e.offsetParent && /klein-base-9b/i.test(e.textContent) && e.textContent.length < 80);
log('klein candidates', cands.map(e => e.tagName + '.' + e.className.slice(0,40) + ' ' + e.textContent.trim().slice(0,40)));
const listEls = [...document.querySelectorAll('[id*=model][class*=list], [class*=model-dropdown], [class*=model-option], [class*=model-item]')].filter(e=>e.offsetParent);
log('list containers', listEls.map(e => e.tagName + '#' + e.id + '.' + e.className.slice(0,50)).slice(0,10));
await shot('p2-10-model-picker');
if (cands[0]) { cands[0].click(); await sleep(800); }
log('after click', inp.value, '| label', document.querySelector('#gen-runtime-label').textContent);
({ value: inp.value });
