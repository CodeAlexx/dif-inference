await sleep(2500);
const q = s => document.querySelector(s);
const set = (id, v) => { const e = q(id); e.value = v; e.dispatchEvent(new Event('input',{bubbles:true})); e.dispatchEvent(new Event('change',{bubbles:true})); };
const inp = q('#gen-model-search');
inp.focus(); inp.click(); inp.dispatchEvent(new Event('focus')); inp.value = 'MiniMax-H3'; inp.dispatchEvent(new Event('input',{bubbles:true})); await sleep(800);
const it = [...document.querySelectorAll('.gen-model-dropdown-item')].find(e => /MiniMax-H3 Base/.test(e.textContent)); it.click(); await sleep(1200);
log('model', inp.value, '|', q('#gen-runtime-label').textContent, '| size', q('#gen-custom-width').value + 'x' + q('#gen-custom-height').value, 'frames', q('#gen-frames').value, 'seconds', q('#gen-seconds').value, 'steps', q('#gen-steps').value, 'quant', q('#gen-video-quant').value);
set('#gen-prompt', 'A lone hiker crests a snowy ridge at golden hour as wind lifts loose powder; boots crunch, the wind gusts, and a distant eagle cries.');
set('#gen-seed', '9001');
await shot('p3-02-h3-before');
const t0 = Date.now();
q('#gen-btn').click(); log('clicked', q('#gen-btn').textContent.trim());
let last = '', status = null, vid = null;
for (let i = 0; i < 600; i++) {
  await sleep(2000);
  if (!vid) { const dirs = await fetch('/v1/video').then(r => r.json()).catch(() => null); }
  const act = (q('#gen-activity-text') || {}).textContent || '';
  const prog = (q('#gen-left-progress-label') || {}).textContent || '';
  const s = act + ' | ' + prog;
  if (s !== last) { log(((Date.now() - t0) / 1000).toFixed(0) + 's', s.slice(0, 160)); last = s; }
  if (i === 40) await shot('p3-03-h3-progress');
  const btn = q('#gen-btn').textContent.trim();
  if (i > 5 && /^Generate/.test(btn) && !/Generating/.test(btn) && !/running|denois|decod|encod/i.test(act)) { break; }
}
await sleep(3000);
await shot('p3-04-h3-after');
const vids = [...document.querySelectorAll('video')].filter(v => v.offsetParent).map(v => v.currentSrc || v.src);
const imgs = [...document.querySelectorAll('img')].filter(e => e.offsetParent && e.naturalWidth > 200).map(e => e.src.slice(0, 100));
log('visible videos', vids, 'imgs', imgs.slice(0, 3));
return { wall_s: Math.round((Date.now() - t0) / 1000), videos: vids, activity: last };
