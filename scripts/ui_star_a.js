await sleep(2500);
const findItem = () => [...document.querySelectorAll('.gen-gallery-date-items > *')].find(e => e.querySelector('img[src*="job-0007"]'));
let el = findItem(); const b = el.querySelector('.gen-thumb-star').textContent;
if (b === '☆') { el.querySelector('.gen-thumb-star').click(); await sleep(800); }
el = findItem(); return { before: b, now: el.querySelector('.gen-thumb-star').textContent };
