"use strict";

// CPU-only project/DOM/request-transport gate. HTTP and timers are intercepted;
// no server or GPU work occurs. Browser visual and decoded-quality gates remain separate.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const plain = value => JSON.parse(JSON.stringify(value));
const context = { console };
vm.createContext(context);
function load(name) { vm.runInContext(fs.readFileSync(path.join(__dirname, name), "utf8"), context, { filename: name }); }
load("h3-project.js");
const C = context.H3ProjectContracts;
const project = C.createProject(), shot = project.shots[0];
shot.brief = "A red toy car drives through a sunlit room.";
shot.lora = [{ name: "H3/style", weight: -0.25 }, { path: "/uploads/second adapter.safetensors", weight: 0.7 }];
shot.controls = [{ path: "/uploads/guide one.mp4", strength: 0.6, start: 0.1, end: 0.9, preprocessor: "prepared" }];
const policy = { lora: { available: true, max_count: 2, max_abs_scale: 3 }, controlnet: { available: true, max_count: 4,
    preprocessors: ['prepared', 'canny'], resize_modes: ['crop', 'pad', 'stretch'],
    defaults: {preprocessor:'prepared', resize_mode:'crop', canny_low:100, canny_high:200, strength:1, start:0, end:1, invert_mask:false} } };
assert.equal(C.featureIssue(shot, policy), "");
shot.controls[0].strength = 11;
assert.equal(C.featureIssue(shot, policy), "", "Mojo accepts every finite ControlNet strength");
shot.controls[0].strength = 0.6;
const request = C.renderRequest(shot);
assert.deepEqual(plain(request.lora), plain(shot.lora));
assert.deepEqual(plain(request.controls), plain(shot.controls));
request.controls[0].strength = 99;
assert.equal(shot.controls[0].strength, 0.6, "request must not alias project state");
const restored = C.normalizeProject(plain(project));
assert.deepEqual(plain(C.renderRequest(restored.shots[0]).controls), plain(shot.controls));
assert.deepEqual(plain(C.renderRequest(restored.shots[0]).lora), plain(shot.lora));
const legacy = C.createProject(); delete legacy.shots[0].lora; delete legacy.shots[0].controls;
assert.deepEqual(plain(C.normalizeProject(plain(legacy)).shots[0].controls), []);
function invalid(change, pattern) { const value = C.copy(shot); change(value); assert.match(C.featureIssue(value, policy), pattern); assert.throws(() => C.renderRequest(value)); }
invalid(s => { s.controls[0].start = 0.95; }, /schedule/);
invalid(s => { s.controls[0].end = NaN; }, /finite/);
invalid(s => { s.lora[0].weight = Infinity; }, /finite/);
invalid(s => { s.lora[0].path = "/ambiguous"; }, /exactly one/);
invalid(s => { s.controls[0].preprocessor = "depth"; }, /prepared or canny/);
invalid(s => { s.controls[0].mask = "/mask.png"; }, /source_path\/mask_path/);
invalid(s => { s.controls[0].source_path = "/source.png"; }, /both source_path and mask_path/);
invalid(s => { s.controls[0].canny_low = 201; }, /thresholds/);
invalid(s => { s.controls[0].resize_mode = "fit"; }, /resize mode/);
invalid(s => { s.controls[0].invert_mask = "true"; }, /boolean/);
invalid(s => { s.controls = Array.from({length:5}, () => C.copy(s.controls[0])); }, /at most 4/);
invalid(s => { s.lora = null; }, /ordered arrays/);
invalid(s => { s.first_frame = "/uploads/first.png"; }, /T2VA only/);
invalid(s => { s.continue_from = "video-0001"; }, /T2VA only/);
invalid(s => { s.references = [C.createReference("image", "/uploads/ref.png")]; }, /T2VA only/);
invalid(s => { s.step_cache = "high"; }, /exact denoise/);
const oversized = C.copy(shot); oversized.lora[0].weight = 4;
assert.match(C.featureIssue(oversized, policy), /configured policy/);
assert.match(C.featureIssue(shot, {}), /unavailable/);
assert.match(C.featureIssue(shot, { ...policy, lora: { available: true } }), /admission policy/);
const invalidImport = plain(project); invalidImport.shots[0].controls = "not an array";
assert.match(C.featureIssue(C.normalizeProject(invalidImport).shots[0]), /ordered arrays/, "bad imported stacks must not disappear");
assert.throws(() => C.createEndlessRun(shot, 20, 10, "Continue naturally"), /T2VA only/, "control must not be silently lost on continuation");
const loraShot = C.copy(shot); loraShot.controls = [];
const run = C.createEndlessRun(loraShot, 20, 10, "Continue naturally");
run.completed_job_ids = ["video-0001"];
assert.deepEqual(plain(C.renderRequest(C.endlessSegmentShot(run, 1)).lora), plain(shot.lora));
const changed = C.copy(loraShot); changed.lora.reverse();
assert.notEqual(C.endlessFingerprint(C.endlessBaseSnapshot(changed, 20, 10, "Continue naturally")), run.fingerprint);
const mediaShot = C.copy(shot);
mediaShot.controls = [C.createControl('/uploads/guide.png', policy.controlnet), C.createControl('/uploads/guide.mp4', policy.controlnet)];
Object.assign(mediaShot.controls[0], {preprocessor:'canny', resize_mode:'pad', source_path:'/uploads/source.png', mask_path:'/uploads/mask.png', invert_mask:true});
C.setControlMedia(mediaShot.controls[0], 'path', {path:'/uploads/guide.png', url:'/preview/guide.png'}, {type:'image/png', name:'guide.png'});
assert.equal(C.featureIssue(mediaShot, policy), '');
assert.deepEqual(plain(C.renderRequest(mediaShot).controls), plain(mediaShot.controls.map(C.controlRequest)));
assert(!JSON.stringify(C.renderRequest(mediaShot).controls).includes('/preview/'));
assert.equal(C.normalizeProject({...plain(project), shots:[plain(mediaShot)]}).shots[0].controls[0]._media.path.url, '/preview/guide.png');

// Minimal DOM adapter parses real rendered attributes and dispatches actual UI
// listeners; it is not a layout engine or a replacement for a browser gate.
const nodes = new Map();
class Node {
    constructor(tag = "div", attrs = {}) {
        this.tagName = tag.toUpperCase(); this.attrs = attrs; this.dataset = {}; this.listeners = {};
        this.value = attrs.value || ""; this.type = attrs.type || (tag === "input" ? "text" : ""); this.style = {};
        for (const [key, value] of Object.entries(attrs)) if (key.startsWith("data-")) this.dataset[key.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = value;
    }
    addEventListener(event, fn) { (this.listeners[event] ||= []).push(fn); }
    fire(event) { for (const fn of this.listeners[event] || []) fn({ target: this }); }
    click() { this.fire("click"); }
}
const panel = new Node(); panel.children = [];
Object.defineProperty(panel, "innerHTML", { set(html) {
    this.html = html; this.children = []; nodes.clear(); nodes.set("panel-h3-studio", this);
    for (const tag of html.matchAll(/<(input|select|button|textarea|div|span|footer)\b([^>]*)>/g)) {
        const attrs = Object.fromEntries(Array.from(tag[2].matchAll(/([\w-]+)="([^"]*)"/g), m => [m[1], m[2].replace(/&quot;/g, '"').replace(/&amp;/g, "&")]));
        const node = new Node(tag[1], attrs); this.children.push(node); if (attrs.id) nodes.set(attrs.id, node);
    }
} });
panel.querySelectorAll = selector => { const attribute = selector.slice(1, -1); return panel.children.filter(n => Object.hasOwn(n.attrs, attribute)); };
nodes.set("panel-h3-studio", panel);
const posts = [], storage = new Map();
let confirmations = 0;
Object.assign(context, {
    document: { getElementById: id => nodes.get(id) || null, createElement: () => new Node(), body: { appendChild: n => nodes.set(n.id, n) } },
    localStorage: { setItem: (key, value) => storage.set(key, value), getItem: key => storage.get(key) },
    window: { confirm: () => { confirmations++; return true; } },
    setTimeout: () => 1, clearTimeout: () => {},
    fetch: async (url, options) => { assert.equal(url, "/v1/video"); assert.equal(options.method, "POST"); posts.push(JSON.parse(options.body)); return { ok: true, text: async () => JSON.stringify({ video_id: "video-0001" }) }; },
    H3AttentionContracts: { resolveBackend: () => "cudnn", definitions: () => [], isAvailable: () => true }
});
load("api.js"); load("h3-studio.js");
const S = context.H3StudioTab;
S.state.project = C.copy(project); S.state.inspectorTab = "features";
S.state.readiness = { candidate_runners: [{ model: "minimax_h3_t2va", available: true, features: policy, step_cache_modes: [{ id: "exact", available: true }] }] };
S.render();
const find = data => { const node = panel.children.find(n => Object.entries(data).every(([k, v]) => n.dataset[k] === v)); assert(node, JSON.stringify(data)); return node; };
const weight = find({ featureKind: "lora", featureIndex: "0", featureField: "weight" });
weight.value = "-0.4"; weight.fire("input");
assert.equal(S.state.project.shots[0].lora[0].weight, -0.4);
find({ featureKind: "lora", featureIndex: "1", featureAction: "move", featureDelta: "-1" }).click();
assert.equal(S.state.project.shots[0].lora[0].path, "/uploads/second adapter.safetensors");
find({ h3Action: "render-shot" }).click();
assert.equal(confirmations, 1);
assert.equal(posts.length, 1);
assert.deepEqual(posts[0].lora, plain(S.state.project.shots[0].lora));
assert.deepEqual(posts[0].controls, plain(S.state.project.shots[0].controls));
assert(storage.has("serenity-h3-current-project-v1"));
S.state.project.shots[0].first_frame = "/uploads/first.png"; S.render(); find({ h3Action: "render-shot" }).click();
assert.equal(posts.length, 1, "unsupported combo must not POST");
assert.match(S.state.status, /T2VA only/);
S.state.project.shots[0].locked = true; S.render(); find({ controlAction: "add" }).click();
assert.equal(S.state.project.shots[0].controls.length, 1, "locked shot cannot mutate controls");
S.state.project.shots[0].locked = false; S.state.project.shots[0].first_frame = ''; S.render();
const preprocessor = find({controlIndex:'0', controlField:'preprocessor'});
preprocessor.value = 'canny'; preprocessor.fire('change');
assert.equal(S.state.project.shots[0].controls[0].preprocessor, 'canny');
assert(find({controlIndex:'0', controlField:'canny_low'}));
for (const [field, value] of [['source_path','/uploads/source.png'], ['mask_path','/uploads/mask.png']]) {
    const node = find({controlIndex:'0', controlField:field}); node.value = value; node.fire('input');
}
const invert = find({controlIndex:'0', controlField:'invert_mask'}); invert.checked = true; invert.fire('change');
assert.equal(S.state.project.shots[0].controls[0].invert_mask, true);
find({controlAction:'add'}).click();
const secondGuide = find({controlIndex:'1', controlField:'path'}); secondGuide.value = '/uploads/second.png'; secondGuide.fire('input');
find({controlAction:'move', controlIndex:'1', controlDelta:'-1'}).click();
assert.equal(S.state.project.shots[0].controls[0].path, '/uploads/second.png');
find({controlAction:'remove', controlIndex:'0'}).click();

// Expose existing closure functions only in this test context; production API
// stays unchanged. Generate uses the same editor and strict request projection.
vm.runInContext(fs.readFileSync(path.join(__dirname, 'generate.js'), 'utf8').replace('buildWorkflow: buildWorkflow,',
    'buildWorkflow: buildWorkflow, buildVideoRequest: buildVideoRequest, renderH3Controls: renderH3Controls,'), context);
const G = context.GenerateTab;
Object.assign(G.state, {arch:'minimax_h3', model:'MiniMax-H3-Compiler', prompt:'A car turns.', h3Mode:'t2va',
    h3Controls:C.copy(mediaShot.controls), h3StepCache:'exact', videoQuant:'bf16', videoStatus:S.state.readiness});
const generated = G.buildVideoRequest(123);
assert.equal(generated.task, 't2va');
assert.deepEqual(plain(generated.controls), plain(mediaShot.controls.map(C.controlRequest)));
G.state.h3StepCache = 'high'; assert.throws(() => G.buildVideoRequest(123), /exact denoise/);
G.state.h3StepCache = 'exact'; G.state.initImagePath = '/uploads/keyframe.png';
assert.throws(() => G.buildVideoRequest(123), /T2VA only/);
G.state.initImagePath = ''; G.state.h3Mode = 'ref2va';
assert.throws(() => G.buildVideoRequest(123), /T2VA only/);
console.log("H3 LoRA/control project, immutable snapshots, DOM event and real API transport: PASS (mock HTTP, no GPU)");
