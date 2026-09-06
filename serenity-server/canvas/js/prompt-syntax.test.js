// node canvas/js/prompt-syntax.test.js — mirrors MojoUI selftest_prompt_syntax (G3a) cases.
const P = require('./prompt-syntax.js');
let fails = 0;
function eq(a, b, msg) { if (JSON.stringify(a) !== JSON.stringify(b)) { fails++; console.log('FAIL', msg, '\n  got', JSON.stringify(a), '\n  want', JSON.stringify(b)); } else console.log('ok  ', msg); }
function ok(c, msg) { if (!c) { fails++; console.log('FAIL', msg); } else console.log('ok  ', msg); }
let r = P.resolve('plain prompt', 1); eq([r.resolved, r.had_syntax, r.notes], ['plain prompt', false, []], 'plain');
r = P.resolve('a (cat:1.3) here', 7); eq([r.resolved, r.notes.length], ['a (cat:1.3) here', 0], 'weight pass-through');
r = P.resolve('a (cat:1.3 here', 7); ok(r.resolved === 'a (cat:1.3 here' && r.notes.some(n => n.includes("unbalanced '('")), 'unbalanced paren noted, verbatim');
r = P.resolve('a (cat:abc) here', 7); ok(r.resolved === 'a (cat:abc) here' && r.notes.some(n => n.includes('not numeric')), 'non-numeric weight noted');
r = P.resolve('sunset <lora:film-v2:0.8> beach', 3); eq([r.resolved, r.loras], ['sunset beach', [{ name: 'film-v2', weight: 0.8 }]], 'lora extract + seam collapse');
r = P.resolve('sunset <lora:detail> beach', 3); eq(r.loras, [{ name: 'detail', weight: 1 }], 'lora default weight');
r = P.resolve('x <lora:> y', 3); ok(r.resolved === 'x <lora:> y' && r.loras.length === 0 && r.notes.length === 1, 'empty lora verbatim');
r = P.resolve('x <lora:name:abc> y', 3); ok(r.resolved === 'x <lora:name:abc> y' && r.loras.length === 0, 'bad lora weight verbatim');
r = P.resolve('x <lora:big:99> y', 3); eq(r.loras, [{ name: 'big', weight: 10 }], 'lora weight clamped to 10'); ok(r.notes.some(n => n.includes('clamped')), 'clamp noted');
const picks = new Set(); let inSet = true;
for (let s = 0; s < 24; s++) { const a = P.resolve('<random:red|green|blue> car', s).resolved; const b = P.resolve('<random:red|green|blue> car', s).resolved; if (a !== b) inSet = false; const w = a.split(' ')[0]; if (!['red', 'green', 'blue'].includes(w)) inSet = false; picks.add(w); }
ok(inSet && picks.size >= 2, 'random deterministic per seed, in option set, >=2 distinct over 24 seeds (' + [...picks] + ')');
r = P.resolve('<random:<random:a|b>|c> z', 5); ok(['a z', 'b z', 'c z'].includes(r.resolved) && r.had_syntax, 'nested random resolves outer-first: ' + r.resolved);
r = P.resolve('x <random:a|b y', 5); ok(r.resolved === 'x <random:a|b y' && r.notes.some(n => n.includes('unterminated')), 'unterminated random verbatim');
r = P.resolve('x <random:> y', 5); ok(r.resolved === 'x <random:> y' && r.notes.some(n => n.includes('empty')), 'empty random verbatim');
r = P.resolve('(a:1.2) <random:x|y> <lora:l:0.5> img', 999); ok(r.had_syntax && r.loras.length === 1 && /^\(a:1\.2\) [xy] img$/.test(r.resolved), 'all three in one prompt: ' + r.resolved);
const m = P.mergeLoras([{ name: 'l', weight: 0.13 }], [{ name: 'l', weight: 0.5 }, { name: 'k', weight: 1 }], []); eq(m, [{ name: 'l', weight: 0.13 }, { name: 'k', weight: 1 }], 'merge: UI weight wins, new tag appended');
r = P.resolve('日本語 <random:桜|梅> の (景色:1.1)', 42); ok(['桜', '梅'].some(w => r.resolved.includes(w)) && r.notes.length === 0, 'utf-8 safe: ' + r.resolved);
console.log(fails ? `FAILED ${fails}` : 'ALL PASS'); process.exit(fails ? 1 : 0);
