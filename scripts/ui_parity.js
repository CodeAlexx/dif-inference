await sleep(2500);
const R = {};
const vis = e => !!(e && e.offsetParent);
const q = s => document.querySelector(s);
const open = id => { const h = q('#' + id); if (h && h.getAttribute('aria-expanded') !== 'true') h.click(); };
{ const inp = q("#gen-model-search"); if (!/klein-base-9b/.test(inp.value)) { inp.focus(); inp.click(); inp.dispatchEvent(new Event("focus")); inp.value = "klein-base-9b"; inp.dispatchEvent(new Event("input",{bubbles:true})); await sleep(800); const it = [...document.querySelectorAll(".gen-model-dropdown-item")].find(e => /flux-2-klein-base-9b/.test(e.textContent)); if (it) { it.click(); await sleep(800); } } }
R.status_bar = { health_text: q('#topbar-status, .topbar-status, [class*=topbar] [class*=status]')?.textContent.trim().slice(0,40), runtime_label: q('#gen-runtime-label').textContent, model_badge: q('#gen-model-badge')?.textContent };
R.model_section = { picker_items: document.querySelectorAll('.gen-model-dropdown-item').length, refresh_btn: vis(q('#gen-model-refresh')) };
R.core = { images: q('#gen-batch')?.value, seed: q('#gen-seed')?.value, seed_shuffle: vis(q('#gen-seed-shuffle')), seed_prev: vis(q('#gen-seed-prev')), seed_random_toggle: vis(q('#gen-seed-random-toggle')), steps_range: vis(q('#gen-steps-range')), cfg_range: vis(q('#gen-cfg-range')) };
open('gen-image-header'); await sleep(200);
R.resolution = { aspect_options: [...q('#gen-aspect-dropdown').options].map(o=>o.textContent.trim()), width_input: !!q('#gen-width-slider'), height_input: !!q('#gen-height-slider'), swap_btn: !![...document.querySelectorAll('button')].find(b => vis(b) && /swap/i.test(b.title + b.textContent + b.id)) };
open('gen-sampling-header'); await sleep(200);
R.sampling = { samplers: [...q('#gen-sampler').options].map(o=>o.textContent.trim()), schedulers: [...q('#gen-scheduler').options].map(o=>o.textContent.trim()), variation_seed: !!q('#gen-variation-seed'), variation_strength: !!q('#gen-variation-strength'), variation_visible: vis(q('#gen-variation-section')) };
open('gen-lora-header'); await sleep(200);
R.lora = { section_visible: vis(q('#gen-lora-section')), picker: !!q('#gen-lora-picker'), capability_note: q('#gen-lora-capability')?.textContent };
open('gen-source-header'); await sleep(200);
R.source_image = { file_input: !!q('#gen-init-image-input'), drop_zone: !!q('#gen-init-drop'), preview: !!q('#gen-init-preview'), clear_btn: !!q('#gen-init-clear'), creativity_slider: !!q('#gen-creativity-range'), creativity_input: !!q('#gen-creativity') };
open('gen-refine-header'); await sleep(200);
R.refine_hires = { rows: [...document.querySelectorAll('#gen-refine-header ~ * .gen-param-row, [data-param-search*=refin], [data-param-search*=upscale]')].map(e => e.getAttribute('data-param-search')).filter(Boolean).slice(0,10) };
open('gen-advanced-sampling-header'); await sleep(200);
R.advanced_sampling = { sigma_shift: !!q('#gen-sigma-shift'), sigma_shift_enabled: !q('#gen-sigma-shift')?.disabled, disabled_rows: [...document.querySelectorAll('[data-param-search*="sigma"], [data-param-search*="eta"], [data-param-search*="clip"], [data-param-search*="restart"]')].map(e => e.getAttribute('data-param-search')), clip_skip_input: !!q('#gen-clip-skip'), note: q('#gen-advanced-sampling-note')?.textContent };
open('gen-grid-header'); await sleep(200);
R.grid = { x: !!q('#gen-grid-x-axis'), y: !!q('#gen-grid-y-axis'), z: !!q('#gen-grid-z-axis'), run: !!q('#gen-grid-run'), axes: [...q('#gen-grid-x-axis').options].map(o=>o.value) };
R.prompt = { textarea: !!q('#gen-prompt'), negative: !!q('#gen-neg-prompt'), negative_visible: vis(q('#gen-neg-prompt')), syntax_note_el: !!q('#gen-prompt-syntax-note'), style_presets: q('#gen-style-preset')?.options.length };
// history: star + reuse
const items = [...document.querySelectorAll('.gen-gallery-date-items > *')].filter(vis);
R.history = { items: items.length, starred_first_toggle: !!q('#gen-starred-first-toggle'), search: vis(q('#gen-gallery-search-input')), upload: vis(q('#gen-gallery-upload-btn')), delete_all: vis(q('#gen-gallery-clear')) };
if (items[0]) { items[0].click(); await sleep(600); const star = [...items[0].querySelectorAll('button,span')].find(e => /[★☆]/.test(e.textContent)); R.history.star_btn = !!star; if (star) { const before = star.textContent; star.click(); await sleep(500); R.history.star_toggled = star.textContent !== before; star.click(); await sleep(300); } const reuse = q('#gen-reuse-params'); R.history.reuse_btn = vis(reuse); R.history.selected_result_seed = q('#gen-seed').value; if (reuse) { const seedBefore = q('#gen-seed').value; q('#gen-seed').value = '1'; reuse.click(); await sleep(500); R.history.reuse_restored_seed = q('#gen-seed').value; } }
// presets roundtrip via product API
const pr = await fetch('/v1/presets', { method: 'POST', headers: {'content-type':'application/json'}, body: JSON.stringify({ name: 'parity-check', params: { model: 'flux-2-klein-base-9b', prompt: 'p', steps: 12, cfg: 3.5, seed: 77, width: 1024, height: 1024 } }) });
const list = await fetch('/v1/presets').then(r=>r.json());
R.presets = { save_status: pr.status, listed: JSON.stringify(list).includes('parity-check'), ui_tab: !![...document.querySelectorAll('button')].find(b => vis(b) && b.textContent.trim() === 'Presets') };
await fetch('/v1/presets/parity-check', { method: 'DELETE' });
// tabs
const tab = name => { const b = [...document.querySelectorAll('[data-tab]')].find(e => e.dataset.tab === name); if (b) b.click(); };
tab('canvas'); await sleep(1200); R.canvas_tab = { konva: !!window.Konva, stage: !!q('.konvajs-content'), tool_buttons: [...document.querySelectorAll('button')].filter(vis).map(b => (b.title || b.textContent).trim()).filter(t => /mask|inpaint|brush|erase|paint|lasso|select|sam/i.test(t)).slice(0,12) };
tab('workflows'); await sleep(1200); const oi = await fetch('/object_info').then(r=>r.json()); R.workflows_tab = { node_types: Object.keys(oi).length, file_inputs: document.querySelectorAll('input[type=file]').length, buttons: [...document.querySelectorAll('button')].filter(vis).map(b => (b.title || b.textContent).trim()).filter(t => /load|import|save|export|template|run|queue/i.test(t)).slice(0,12), templates: (await fetch('/templates').then(r=>r.json()).catch(()=>[])).length };
tab('models'); await sleep(1000); R.models_tab = { rows: document.querySelectorAll('[class*=model-card], [class*=model-row], tr').length };
tab('queue'); await sleep(800); R.queue_tab = { cancel_current: !!q('#queue-cancel-current'), pending_list: !!q('#queue-pending-list, [id*=pending]') };
tab('settings'); await sleep(800); R.settings_tab = { text: document.body.innerText.slice(0, 0), has_model_dirs: /Model Search Director|model director|folder/i.test(document.body.innerText), has_output: /output/i.test(document.body.innerText) };
await shot('p2-11-settings');
tab('generate'); await sleep(500);
log('PARITY', R);
return R;
