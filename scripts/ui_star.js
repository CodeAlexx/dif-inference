await sleep(2500);
const findItem = () => [...document.querySelectorAll('.gen-gallery-date-items > *')].find(e => e.querySelector('img[src*="job-0007"]'));
let el = findItem(); log('item found', !!el);
const glyphBefore = el.querySelector('.gen-thumb-star').textContent;
el.querySelector('.gen-thumb-star').click(); await sleep(800);
el = findItem();
const glyphAfter = el.querySelector('.gen-thumb-star').textContent;
const starredClass = el.querySelector('.gen-thumb-star').className;
log('glyph before/after', glyphBefore, glyphAfter, 'class', starredClass);
// reload the page: does the star survive?
location.reload(); await sleep(3500);
el = findItem();
const glyphReload = el ? el.querySelector('.gen-thumb-star').textContent : 'no item';
log('after reload', glyphReload);
if (el) { el.querySelector('.gen-thumb-star').click(); await sleep(500); }
return { before: glyphBefore, after: glyphAfter, afterReload: glyphReload };
