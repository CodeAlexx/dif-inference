/* prompt-syntax.js — SerenityUI prompt-syntax parity for the web Generate tab.
 *
 * Port of MojoUi/mojoui/app/prompt_syntax.mojo (the desktop app's parser).
 * Three syntaxes, resolved at SUBMIT time against the concrete job seed; the
 * original text is preserved as `prompt_raw`, the resolved text goes in
 * `prompt`:
 *
 *   (text:1.3)      attention weighting — PASSED THROUGH verbatim; only paren
 *                   balance and a numeric weight tail are validated (soft notes).
 *   <lora:name:0.8> extracted into a LoRA request (weight optional, default 1.0,
 *                   clamped to [-10, 10] with a note); the tag is removed. The
 *                   caller merges into the UI LoRA stack (dedupe by name — the UI
 *                   stack wins).
 *   <random:a|b|c>  uniform pick seeded by the JOB seed (splitmix64): deterministic
 *                   per seed. Nested <random:> resolves on the next pass
 *                   (outer-first, left-to-right, max 16 passes).
 *
 * Malformed syntax is NEVER fatal: the span passes through verbatim with a
 * human-readable note. Works in the browser (window.SerenityPromptSyntax) and
 * under Node (module.exports) so prompt-syntax.test.js can gate it.
 */
(function (root, factory) {
    var api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    root.SerenityPromptSyntax = api;
})(typeof window !== 'undefined' ? window : globalThis, function () {
    'use strict';

    // splitmix64 over BigInt, bit-for-bit the Mojo _mix64.
    var MASK = (1n << 64n) - 1n;
    function mix64(state) {
        var z = state & MASK;
        z = ((z ^ (z >> 30n)) * 0xBF58476D1CE4E5B9n) & MASK;
        z = ((z ^ (z >> 27n)) * 0x94D049BB133111EBn) & MASK;
        return (z ^ (z >> 31n)) & MASK;
    }

    // [+-]?digits[.digits], surrounding spaces tolerated; null on junk.
    function parseFloatStrict(s) {
        var m = /^[ \t]*([+-]?)(\d*)(?:\.(\d*))?[ \t]*$/.exec(s);
        if (!m) return null;
        var intPart = m[2] || '', frac = m[3];
        if (!intPart && !(frac && frac.length)) return null;
        var v = Number((intPart || '0') + '.' + (frac || '0'));
        return m[1] === '-' ? -v : v;
    }

    function trim(s) { return s.replace(/^[ \t]+|[ \t]+$/g, ''); }

    // one outer-first, left-to-right pass over <random:...>
    function resolveRandomsPass(text, rngBox, notes) {
        var out = '', i = 0, n = text.length, changed = false;
        while (i < n) {
            if (text.substr(i, 8) !== '<random:') { out += text[i]; i++; continue; }
            var bodyLo = i + 8, j = bodyLo, depth = 1, close = -1;
            while (j < n) {
                if (text[j] === '<') depth++;
                else if (text[j] === '>') { depth--; if (depth === 0) { close = j; break; } }
                j++;
            }
            if (close < 0) {
                notes.push('unterminated <random:...> passed through verbatim');
                out += text.slice(i); i = n; break;
            }
            if (close === bodyLo) {
                notes.push('empty <random:> passed through verbatim');
                out += text.slice(i, close + 1); i = close + 1; continue;
            }
            var options = [], segLo = bodyLo, d2 = 0;
            for (var k = bodyLo; k <= close; k++) {
                if (k === close || (text[k] === '|' && d2 === 0)) { options.push(text.slice(segLo, k)); segLo = k + 1; }
                else if (text[k] === '<') d2++;
                else if (text[k] === '>') d2--;
            }
            rngBox.state = mix64(rngBox.state);
            var pick = Number(rngBox.state % BigInt(options.length));
            out += options[pick];
            changed = true;
            i = close + 1;
        }
        return { text: out, changed: changed };
    }

    function extractLoras(text, loras, notes) {
        var out = '', i = 0, n = text.length;
        while (i < n) {
            if (text.substr(i, 6) !== '<lora:') { out += text[i]; i++; continue; }
            var bodyLo = i + 6, j = bodyLo, close = -1, nested = false;
            while (j < n) {
                if (text[j] === '>') { close = j; break; }
                if (text[j] === '<') { nested = true; break; }
                j++;
            }
            if (close < 0 || nested) {
                notes.push('malformed <lora:...> passed through verbatim');
                out += text[i]; i++; continue;
            }
            var body = text.slice(bodyLo, close);
            var name = trim(body), weight = 1.0, weightOk = true;
            var lastColon = body.lastIndexOf(':');
            if (lastColon >= 0) {
                var w = parseFloatStrict(body.slice(lastColon + 1));
                if (w !== null) { name = trim(body.slice(0, lastColon)); weight = w; }
                else weightOk = false;
            }
            if (!name.length || !weightOk) {
                notes.push('malformed <lora:' + body + '> passed through verbatim');
                out += text.slice(i, close + 1); i = close + 1; continue;
            }
            if (weight < -10) { notes.push('<lora:' + name + ':' + weight + '> weight clamped to -10 (daemon range [-10,10])'); weight = -10; }
            else if (weight > 10) { notes.push('<lora:' + name + ':' + weight + '> weight clamped to 10 (daemon range [-10,10])'); weight = 10; }
            loras.push({ name: name, weight: weight });
            i = close + 1;
            // collapse the seam: tag removal must not leave a double space
            if (out.length > 0 && out[out.length - 1] === ' ') { while (i < n && text[i] === ' ') i++; }
            else if (out.length === 0) { while (i < n && text[i] === ' ') i++; }
        }
        return out;
    }

    // (text:1.3) — validate only, never rewrite
    function validateWeightSyntax(text, notes) {
        var depth = 0, starts = [], colons = [];
        for (var i = 0; i < text.length; i++) {
            var c = text[i];
            if (c === '(') { depth++; starts.push(i); colons.push(-1); }
            else if (c === ':' && depth > 0) colons[colons.length - 1] = i;
            else if (c === ')') {
                if (depth === 0) { notes.push("unbalanced ')' in prompt — weights passed through"); return; }
                depth--;
                var gs = starts.pop(), gc = colons.pop();
                if (gc >= 0 && parseFloatStrict(text.slice(gc + 1, i)) === null)
                    notes.push("weight tag '" + text.slice(gs, i + 1) + "' not numeric — passed through");
            }
        }
        if (depth !== 0) notes.push("unbalanced '(' in prompt — weights passed through");
    }

    /** Resolve all prompt syntax against the concrete job seed. Never throws. */
    function resolve(prompt, seed) {
        var notes = [], loras = [];
        var s = Number(seed) || 0;
        var rngBox = { state: BigInt(Math.trunc(s < 0 ? -s : s)) & MASK };
        var text = String(prompt == null ? '' : prompt), anyRandom = false;
        for (var pass = 0; pass < 16; pass++) {
            var r = resolveRandomsPass(text, rngBox, notes);
            text = r.text;
            if (!r.changed) break;
            anyRandom = true;
        }
        text = extractLoras(text, loras, notes);
        validateWeightSyntax(text, notes);
        return { resolved: text, loras: loras, notes: notes, had_syntax: anyRandom || loras.length > 0 };
    }

    /** Merge tag LoRAs into the UI stack: dedupe by name, UI stack weight wins. */
    function mergeLoras(uiLoras, tagLoras, notes) {
        var out = uiLoras.slice();
        tagLoras.forEach(function (t) {
            var hit = out.find(function (u) { return u.name === t.name; });
            if (hit) { if (notes) notes.push('<lora:' + t.name + '> already in the LoRA stack — UI weight ' + hit.weight + ' wins'); }
            else out.push({ name: t.name, weight: t.weight });
        });
        return out;
    }

    return { resolve: resolve, mergeLoras: mergeLoras, parseFloatStrict: parseFloatStrict, joinNotes: function (n) { return n.join('; '); } };
});
