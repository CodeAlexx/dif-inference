await sleep(3000);
const findItem = () => [...document.querySelectorAll('.gen-gallery-date-items > *')].find(e => e.querySelector('img[src*="job-0007"]'));
let el = findItem(); const g = el ? el.querySelector('.gen-thumb-star').textContent : 'no item';
const firstIsStarred = [...document.querySelectorAll('.gen-gallery-date-items > *')][0]?.querySelector('.gen-thumb-star')?.className.includes('starred');
if (el && g === '★') { el.querySelector('.gen-thumb-star').click(); await sleep(500); }
return { afterFreshLoad: g, firstItemStarred: firstIsStarred };
