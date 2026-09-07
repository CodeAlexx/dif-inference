#!/usr/bin/env node
"use strict";

// Compare the port to the actual Mojo product source. No model execution and
// no altered graph defaults; network calls are intercepted at the boundary.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const repo = path.resolve(__dirname, "..");
const mojo = process.env.MOJO_SOURCE_ROOT || "/home/alex/mojodiffusion";
const plain = value => JSON.parse(JSON.stringify(value));
const canvas = root => path.join(root, "serenity-server", "canvas");
for (const file of ["js/h3-control.js", "js/h3-control-workflow.js", "css/h3-control.css"]) {
    // The user's original clip is 832x480. Preserve every source behavior
    // while exposing that explicitly requested geometry in this compiler UI.
    let expected = fs.readFileSync(path.join(canvas(mojo), file), "utf8");
    if (file === "js/h3-control.js") expected = expected
        .replace("    var h3ResolutionPresets = {\n", "    var h3ResolutionPresets = {\n        '832x480': { width: 832, height: 480 },\n")
        .replace('                \'<option value="1536x672">', '                \'<option value="832x480">832×480</option>\' +\n                \'<option value="1536x672">')
        .replace("Six supported H3 presets only. ", "")
        // This is the Diffusion Compiler product, not the Mojo stack. The Mojo
        // tree is a source reference, so the compiler UI must not claim to be
        // "native Mojo" inference in text the operator reads.
        .replace("Native Mojo H3 inference", "Diffusion Compiler H3 inference")
        .replace("Reading installed native Mojo H3 models", "Reading installed Diffusion Compiler H3 models")
        .replace("Mojo inference outputs", "Compiler inference outputs")
        .replace("No Mojo video outputs found yet.", "No compiler video outputs found yet.");
    if (file === "js/h3-control-workflow.js") expected = expected
        .replace("    var h3ResolutionPresets = {\n", "    var h3ResolutionPresets = {\n        '832x480': true,\n")
        .replace("one of the six supported presets", "one of the supported presets")
        .replace("// lowered by the shared API into one native Mojo H3 ControlNet request.",
                 "// lowered by the shared API into one native compiler H3 ControlNet request.");
    assert.equal(fs.readFileSync(path.join(canvas(repo), file), "utf8"), expected,
        `${file} changed beyond the requested original-video resolution`);
}
function load(root) {
    const context = vm.createContext({console, FormData: function () {},
        fetch() { throw new Error("source parity must not submit a job"); },
        SerenityWS: {getClientId() { return "source-parity"; }}});
    for (const file of ["h3-control-workflow.js", "api.js"])
        vm.runInContext(fs.readFileSync(path.join(canvas(root), "js", file), "utf8"), context);
    return context;
}
const source = load(mojo), target = load(repo);
const sizes = [[1536,672], [1344,768], [1024,768], [768,768], [768,1024], [768,1344]];
let cases = 0;
for (const [width, height] of sizes) for (const paired of [false, true]) {
    const params = {model:"base", controlNet:"union", clipName:"text", videoVae:"video", audioVae:"audio",
        prompt:"Use exactly this prompt.", width, height, durationSeconds:5, steps:20, seed:9003,
        outputFormat:"mp4", sourceMedia:paired ? "/source.mp4" : "", maskMedia:paired ? "/mask.png" : "",
        invertMask:paired, controls:[
            {controlMedia:"/guide.mp4", preprocessor:"canny", resizeMode:"pad", cannyLow:90, cannyHigh:180,
                strength:0.6, startPercent:0.1, endPercent:0.9},
            {controlMedia:"/guide.png", preprocessor:"prepared", resizeMode:"stretch",
                strength:-0.25, startPercent:0, endPercent:1}], loras:[]};
    const expected = plain(source.H3ControlWorkflow.build(params));
    const actual = plain(target.H3ControlWorkflow.build(params));
    assert.deepEqual(actual, expected, "ControlNet graph changed");
    const request = plain(target.SerenityAPI.videoRequestFromWorkflow(actual));
    assert.deepEqual(request, plain(source.SerenityAPI.videoRequestFromWorkflow(expected)), "request changed");
    assert.equal(request.width, width); assert.equal(request.height, height);
    assert.equal(request.steps, 20); assert.equal(request.seed, 9003);
    assert.equal(request.attention_backend, "ck-int8");
    assert.equal(request.quant, "int8");
    assert.equal(request.controls[0].source_path, params.sourceMedia);
    assert.equal(request.controls[0].mask_path, params.maskMedia);
    assert.equal(request.controls[0].invert_mask, paired);
    assert.equal(request.controls[1].strength, -0.25);
    cases++;
}
const original = target.H3ControlWorkflow.build({model:"base",controlNet:"union",videoVae:"video",audioVae:"audio",
    prompt:"Replace only the man with a polar bear.",width:832,height:480,durationSeconds:5,steps:20,seed:9003,
    controlMedia:"/original.mp4",preprocessor:"canny",sourceMedia:"/original.mp4",maskMedia:"/mask.png"});
const originalRequest = plain(target.SerenityAPI.videoRequestFromWorkflow(original));
assert.equal(originalRequest.task,"controlnet");
assert.equal(originalRequest.width,832); assert.equal(originalRequest.height,480);
assert.equal(originalRequest.controls[0].path,"/original.mp4");
assert.equal(originalRequest.controls[0].source_path,"/original.mp4");
assert.equal(originalRequest.controls[0].mask_path,"/mask.png");
assert.equal(originalRequest.quant,"int8"); assert.equal(originalRequest.attention_backend,"ck-int8");
console.log(`H3 ControlNet source requests: PASS (${cases} source cases plus original-video 832x480, no GPU)`);
