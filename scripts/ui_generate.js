await sleep(2500);
const prompt = document.querySelector('#gen-prompt');
prompt.value = 'a small wooden rowboat on a misty lake at sunrise, soft pastel sky';
prompt.dispatchEvent(new Event('input', {bubbles:true})); prompt.dispatchEvent(new Event('change', {bubbles:true}));
const steps = document.querySelector('#gen-steps'); steps.value = '12'; steps.dispatchEvent(new Event('input',{bubbles:true})); steps.dispatchEvent(new Event('change',{bubbles:true}));
const seed = document.querySelector('#gen-seed'); seed.value = '4242'; seed.dispatchEvent(new Event('input',{bubbles:true})); seed.dispatchEvent(new Event('change',{bubbles:true}));
log('model field', document.querySelector('#gen-model-search').value, 'steps', document.querySelector('#gen-steps').value);
await shot('p2-02-before-generate');
const jobsBefore = await fetch('/v1/jobs').then(r=>r.json());
document.querySelector('#gen-btn').click();
log('clicked Generate');
let last = '';
for (let i = 0; i < 120; i++) {
  await sleep(1000);
  const jobs = await fetch('/v1/jobs').then(r=>r.json());
  const j = jobs.find(x => !jobsBefore.some(y => y.id === x.id)) || jobs[0];
  const s = j ? `${j.id} ${j.state} ${j.step}/${j.total}` : 'no job';
  if (s !== last) { log(s); last = s; }
  if (i === 25) await shot('p2-03-progress');
  if (j && ['done','failed','cancelled'].includes(j.state)) { await sleep(2500); await shot('p2-04-after'); 
    const imgs = [...document.querySelectorAll('img')].filter(e=>e.offsetParent && e.naturalWidth>200).map(e=>e.src.slice(0,120));
    log('visible large images', imgs);
    const hist = [...document.querySelectorAll('[class*=gallery] [class*=item], [class*=history] [class*=item]')].length;
    log('gallery items', hist);
    ({ job: j, imgs }); break; }
}
