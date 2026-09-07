//! One deployment document for the launcher, workers and readiness checks.
//! `extends` is relative to its JSON file. Arrays replace; objects merge.
//! ${repo}, ${config}, ${home} and ${dotted.config.key} are literal references,
//! never shell expressions. Bad references and cycles are errors.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn repository_root() -> PathBuf {
    if let Some(p) = std::env::var_os("SERENITY_REPO_ROOT") { return p.into(); }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.ancestors().find(|p| p.join("config/difc.json").is_file()) {
            return p.to_path_buf();
        }
    }
    // Never fall back to the machine path where this executable was compiled.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.ancestors().find(|p| p.join("config/difc.json").is_file()).unwrap_or(&cwd).to_path_buf()
}

pub fn config_path() -> PathBuf {
    let path = std::env::var_os("DIFC_CONFIG").map(PathBuf::from)
        .unwrap_or_else(|| repository_root().join("config/difc.json"));
    path.canonicalize().unwrap_or(path)
}

pub fn get<'a>(doc: &'a Value, key: &str) -> Result<&'a Value, String> {
    key.split('.').try_fold(doc, |v, part| v.get(part))
        .ok_or_else(|| format!("missing configuration key: {key}"))
}

pub fn string<'a>(doc: &'a Value, key: &str) -> Result<&'a str, String> {
    get(doc, key)?.as_str().filter(|s| !s.is_empty())
        .ok_or_else(|| format!("configuration key {key} must be a nonempty string"))
}

fn merge(base: &mut Value, child: Value) {
    if let (Some(a), Some(b)) = (base.as_object_mut(), child.as_object()) {
        for (k, v) in b { merge(a.entry(k.clone()).or_insert(Value::Null), v.clone()); }
    } else { *base = child; }
}

fn read_tree(path: &Path, stack: &mut Vec<PathBuf>) -> Result<Value, String> {
    let path = path.canonicalize().map_err(|e| format!("config {}: {e}", path.display()))?;
    if stack.contains(&path) || stack.len() >= 32 { return Err(format!("config extends cycle/depth at {}", path.display())); }
    stack.push(path.clone());
    let mut child: Value = serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("config {}: {e}", path.display()))?;
    let obj = child.as_object_mut().ok_or("config must be a JSON object")?;
    let mut base = if let Some(parent) = obj.remove("extends") {
        let name = parent.as_str().ok_or("extends must be a file path string")?;
        read_tree(&path.parent().unwrap().join(name), stack)?
    } else { Value::Null };
    merge(&mut base, child);
    stack.pop();
    Ok(base)
}

fn expand(v: &Value, root: &Value, vars: &Value, stack: &mut Vec<String>) -> Result<Value, String> {
    Ok(match v {
        Value::String(s) => {
            let mut result = String::new();
            let mut rest = s.as_str();
            while let Some(start) = rest.find("${") {
                result.push_str(&rest[..start]);
                let tail = &rest[start + 2..];
                let end = tail.find('}').ok_or_else(|| format!("unterminated config reference in {s}"))?;
                let key = &tail[..end];
                if stack.iter().any(|k| k == key) || stack.len() >= 32 { return Err(format!("config reference cycle/depth: {key}")); }
                stack.push(key.into());
                let source = vars.get(key).map(Ok).unwrap_or_else(|| get(root, key))?;
                let resolved = expand(source, root, vars, stack)?;
                result.push_str(resolved.as_str().ok_or_else(|| format!("reference {key} is not a string"))?);
                stack.pop();
                rest = &tail[end + 1..];
            }
            result.push_str(rest);
            Value::String(result)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| expand(x, root, vars, stack)).collect::<Result<_, _>>()?),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| Ok((k.clone(), expand(v, root, vars, stack)?))).collect::<Result<_, String>>()?),
        _ => v.clone(),
    })
}

pub fn load(path: &Path, repo: &Path) -> Result<Value, String> {
    let root = read_tree(path, &mut Vec::new())?;
    let vars = serde_json::json!({
        "repo": repo.canonicalize().map_err(|e| e.to_string())?,
        "config": path.canonicalize().map_err(|e| e.to_string())?.parent().unwrap(),
        "home": std::env::var("HOME").map_err(|_| "HOME is unavailable for config expansion")?
    });
    let doc = expand(&root, &root, &vars, &mut Vec::new())?;
    if doc["schema_version"] != 1 { return Err("unsupported or missing config schema_version (expected 1)".into()); }
    validate(&doc)?;
    Ok(doc)
}

pub fn validate(doc: &Value) -> Result<(), String> {
    for key in ["compiler_build", "gpu_lock", "memory_max", "memory_swap_max", "desktop_reserve",
        "server.binary", "server.worker", "server.model_root", "server.output_dir",
        "runtime.memory_wrapper", "runtime.memory_high", "runtime.cuda_cache"] {
        string(doc, key)?;
    }
    if !doc["server"]["port"].as_u64().is_some_and(|v| v > 0 && v <= u16::MAX as u64) {
        return Err("server.port must be an integer from 1 through 65535".into());
    }
    if !doc["gpu_lock_wait_seconds"].as_u64().is_some_and(|v| v > 0) {
        return Err("gpu_lock_wait_seconds must be a positive integer".into());
    }
    if let Some(h3) = doc.get("minimax_h3") {
        for key in ["profile.width", "profile.height", "profile.frames", "profile.fps", "profile.steps",
            "profile.blocks", "profile.timestep_tables", "policy.min_free_mib", "policy.int8_mlp_chunk_rows",
            "policy.streamed_stage_threads", "policy.low_vram_total_mib", "policy.low_vram_min_free_mib"] {
            if !get(h3, key)?.as_u64().is_some_and(|n| n > 0 && n <= u32::MAX as u64) {
                return Err(format!("minimax_h3.{key} must be a positive uint32"));
            }
        }
        if !h3["resident_layers"].as_u64().is_some_and(|n| Some(n) <= h3["profile"]["blocks"].as_u64()) {
            return Err("minimax_h3.resident_layers must be between zero and profile.blocks".into());
        }
        if let Some(modes) = h3["profile"].get("step_cache_modes") {
            if !modes.as_array().is_some_and(|modes| !modes.is_empty()
                && modes.iter().all(|mode| matches!(mode.as_str(), Some("exact" | "high")))
                && modes.contains(&h3["profile"]["step_cache"])) {
                return Err("minimax_h3.profile.step_cache_modes must contain the default and only exact/high".into());
            }
        }
    }
    let mut aliases = std::collections::HashSet::new();
    if let Some(profiles) = doc["image_profiles"].as_object() {
        for (key, profile) in profiles {
            for alias in profile["aliases"].as_array().ok_or_else(|| format!("{key}.aliases must be an array"))? {
                let alias = alias.as_str().filter(|s| !s.is_empty()).ok_or("model alias must be nonempty")?;
                if !aliases.insert(alias.to_ascii_lowercase()) { return Err(format!("duplicate model alias: {alias}")); }
            }
            for field in ["width", "height", "steps"] {
                if !profile["defaults"][field].as_u64().is_some_and(|v| v > 0 && v <= u32::MAX as u64) {
                    return Err(format!("image_profiles.{key}.defaults.{field} must be a positive uint32"));
                }
            }
            if !profile["defaults"]["cfg"].as_f64().is_some_and(|v| v.is_finite() && v >= 0.0) {
                return Err(format!("image_profiles.{key}.defaults.cfg must be finite and nonnegative"));
            }
        }
    }
    Ok(())
}

pub fn h3_contract(doc: &Value, request: &Value) -> Result<(), String> {
    let task = request["task"].as_str().unwrap_or("");
    let selected = h3_task_document(doc, task)?;
    let doc = &selected;
    let profile = get(doc, "minimax_h3.profile")?;
    for key in ["width", "height", "frames", "fps", "steps"] {
        // Continuation frames are derived from the overlap window, so that rule
        // runs before the generic geometry check below.
        if key == "frames" && task == "continue" {
            let overlap = request["motion_context_frames"].as_i64().ok_or("H3 continuation needs a numeric overlap")?;
            if !doc["minimax_h3"]["motion"]["windows"].as_array().is_some_and(|a| a.contains(&serde_json::json!(overlap))) {
                return Err("H3 motion overlap is not in configured windows".into());
            }
            let frames = profile["frames"].as_i64().filter(|n| (5..=10000).contains(n)).ok_or("Invalid H3 delivery frames")?;
            if !(5..=39).contains(&overlap) { return Err("Unsupported H3 native motion overlap".into()); }
            let expected = ((frames + overlap - 5 + 16) / 17) * 17 + 5;
            if request[key].as_i64() != Some(expected) { return Err(format!("H3 continuation frames must be {expected}")); }
            continue;
        }
        // Geometry is authored per request, not sealed to the fixture profile.
        // The denoiser program and bundle are rebuilt per request from the actual
        // row counts, the video VAE decodes through the tiled program, and audio
        // is geometry-independent. ControlNet was already exempt here and has
        // been rendering off profile (video-0044 / video-0051 both decoded
        // 1344x768x120 through this same chain); sealing every other task meant
        // H3 Studio could only ever queue the fixture shape.
        //
        // FPS stays sealed: output framing and the 17-frame internal alignment
        // are derived from it.
        if key != "fps" {
            if request[key].as_i64().is_none_or(|n| n <= 0) {
                return Err(format!("H3 {key} must be a positive integer"));
            }
            continue;
        }
        if request[key].as_i64().is_none() || request[key] != profile[key] {
            return Err(format!("H3 {key}={} does not match configured sealed profile ({})", request[key], profile[key]));
        }
    }
    if !h3_task_supported(doc, request["task"].as_str().unwrap_or("")) {
        return Err("H3 task is not in minimax_h3.profile.tasks".into());
    }
    if request["attention"] != profile["attention"]
        && !(task == "controlnet" && request["attention"] == "ck-int8") {
        return Err("H3 attention must match minimax_h3.profile.attention".into());
    }
    let cache_allowed = match profile.get("step_cache_modes") {
        Some(modes) => modes.as_array().is_some_and(|modes| modes.contains(&request["step_cache"])),
        None => request["step_cache"] == profile["step_cache"],
    };
    if !cache_allowed {
        return Err("H3 step_cache is not enabled in minimax_h3.profile.step_cache_modes".into());
    }
    if !profile["quant_modes"].as_array().is_some_and(|modes| modes.contains(&request["quant"])) {
        return Err("H3 quant is not in minimax_h3.profile.quant_modes".into());
    }
    Ok(())
}

/// Task variants are separate deployment documents, not aliases for Base's
/// weights. Resolve their templates after inheritance so checkpoint-dependent
/// paths cannot retain a different task's values.
pub fn h3_task_document(doc: &Value, task: &str) -> Result<Value, String> {
    match doc["minimax_h3"]["task_configs"].get(task) {
        None | Some(Value::Null) => Ok(doc.clone()),
        Some(Value::String(path)) if !path.is_empty() => {
            let selected = load(Path::new(path), &repository_root())?;
            if !h3_task_supported(&selected, task) {
                return Err(format!("H3 task config does not declare task={task}"));
            }
            Ok(selected)
        },
        _ => Err(format!("minimax_h3.task_configs.{task} must be a config file path")),
    }
}

pub fn h3_task_supported(doc: &Value, task: &str) -> bool {
    let profile = &doc["minimax_h3"]["profile"];
    match profile.get("tasks") {
        Some(tasks) => tasks.as_array().is_some_and(|a| a.iter().any(|v| v.as_str() == Some(task))),
        None => profile["task"].as_str() == Some(task),
    }
}

pub fn h3_task_missing(doc: &Value, task: &str, quant: &str) -> Vec<String> {
    let selected = match h3_task_document(doc, task) {
        Ok(selected) => selected,
        Err(error) => return vec![error],
    };
    let doc = &selected;
    if !h3_task_supported(doc, task) || !matches!(task, "t2va" | "i2va" | "l2va" | "fl2va" | "ref2va" | "continue" | "controlnet") {
        return vec![format!("native H3 task={task} is not configured/implemented")];
    }
    if !doc["minimax_h3"]["profile"]["quant_modes"].as_array()
        .is_some_and(|modes| modes.iter().any(|v| v.as_str() == Some(quant))) {
        return vec![format!("native H3 quant={quant} is not configured")];
    }
    // The source ControlNet UI's int8 route prepares its groupwise cache
    // from the BF16 checkpoint; it does not consume a ConvRot/W8A8 pack.
    let mut missing = h3_missing(doc, if task == "controlnet" && quant == "int8" { "bf16" } else { quant });
    if task == "continue" {
        for key in ["stage_script", "modulation_cache"] {
            let key = format!("minimax_h3.motion.{key}");
            match string(doc, &key) {
                Ok(path) if Path::new(path).is_file() => {},
                Ok(path) => missing.push(format!("{key}: {path}")),
                Err(error) => missing.push(error),
            }
        }
        if let Ok(build) = string(doc, "compiler_build") {
            for tool in ["difh3motion", "difimport"] {
                let path = Path::new(build).join(tool);
                if !executable(&path) { missing.push(format!("missing motion tool: {}", path.display())); }
            }
        }
    }
    if matches!(task, "i2va" | "l2va" | "fl2va" | "ref2va") {
        for key in ["stage_script", "vision_program", "vision_bundle", "encoder_program", "encoder_bundle", "modulation_cache"] {
            let key = format!("minimax_h3.keyframes.{key}");
            match string(doc, &key) {
                Ok(p) if std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0) => {},
                Ok(p) => missing.push(format!("{key}: {p}")),
                Err(e) => missing.push(e),
            }
        }
        match doc["minimax_h3"]["keyframes"]["required_tools"].as_array() {
            Some(tools) if !tools.is_empty() => for tool in tools {
                let p = tool.as_str().unwrap_or("");
                if !executable(Path::new(p)) { missing.push(format!("missing keyframe executable: {p}")); }
            },
            _ => missing.push("minimax_h3.keyframes.required_tools must be a nonempty array".into()),
        }
    }
    if task == "ref2va" {
        for key in ["minimax_h3.references.stage_script", "minimax_h3.references.preparation_runner"] {
            match string(doc, key) {
                Ok(path) if Path::new(path).is_file() => {},
                Ok(path) => missing.push(format!("{key}: {path}")),
                Err(error) => missing.push(error),
            }
        }
        if doc["minimax_h3"]["references"]["kinds"].as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "audio")) {
            for field in ["tool", "ffmpeg", "ffprobe"] {
                let key = format!("minimax_h3.references.audio_encoder.{field}");
                match string(doc, &key) {
                    Ok(path) if executable(Path::new(path)) => {},
                    Ok(path) => missing.push(format!("{key} is not executable: {path}")),
                    Err(error) => missing.push(error),
                }
            }
            for field in ["config", "checkpoint", "modulation_cache"] {
                let key = format!("minimax_h3.references.audio_encoder.{field}");
                match string(doc, &key) {
                    Ok(path) if std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0) => {},
                    Ok(path) => missing.push(format!("{key}: {path}")),
                    Err(error) => missing.push(error),
                }
            }
        }
    }
    missing
}

pub fn current() -> Result<&'static Value, String> {
    static DOC: OnceLock<Result<Value, String>> = OnceLock::new();
    DOC.get_or_init(|| load(&config_path(), &repository_root())).as_ref().map_err(Clone::clone)
}

pub fn image_profile_in<'a>(doc: &'a Value, identity: &str) -> Option<(&'a str, &'a Value)> {
    let lower = identity.trim().to_ascii_lowercase();
    let name = Path::new(&lower).file_name()?.to_str()?;
    let name = name.strip_suffix(".safetensors").unwrap_or(name);
    doc.get("image_profiles")?.as_object()?.iter().find_map(|(key, p)| {
        p["aliases"].as_array()?.iter().any(|alias| alias.as_str().is_some_and(|s| s.eq_ignore_ascii_case(name)))
            .then_some((key.as_str(), p))
    })
}

pub fn image_profile(identity: &str) -> Option<(&'static str, &'static Value)> {
    image_profile_in(current().ok()?, identity)
}

/// Uses exactly the artifacts named by the runner; no fabricated fallback paths.
pub fn h3_missing(doc: &Value, quant: &str) -> Vec<String> {
    let mut missing = Vec::new();
    let keys = ["minimax_h3.runner", "minimax_h3.processor", "minimax_h3.transformer_index",
        "minimax_h3.text_encoder_index", "minimax_h3.conditioner_bundle", "minimax_h3.denoiser_bundle",
        "minimax_h3.video_program", "minimax_h3.video_bundle", "minimax_h3.audio_program",
        "minimax_h3.audio_bundle", "minimax_h3.modulation_cache"];
    let int8_key = if doc["minimax_h3"]["int8_route"] == "w8a8" {
        "minimax_h3.w8a8_cache"
    } else { "minimax_h3.convrot_int8" };
    for key in keys.iter().copied().chain((quant != "bf16").then_some(int8_key)) {
        match string(doc, key) {
            Ok(p) if std::fs::metadata(p).is_ok_and(|m| m.is_dir() || m.len() > 0) => {},
            Ok(p) => missing.push(format!("{key}: {p}")),
            Err(e) => missing.push(e),
        }
    }
    for key in ["minimax_h3.transformer_index", "minimax_h3.text_encoder_index"] {
        if let Ok(path) = string(doc, key) {
            if let Ok(text) = std::fs::read_to_string(path) {
                match serde_json::from_str::<Value>(&text).ok().and_then(|v| v["weight_map"].as_object().cloned()) {
                    Some(map) if !map.is_empty() => {
                        let files: std::collections::BTreeSet<_> = map.values().filter_map(Value::as_str).collect();
                        for file in files {
                            let shard = Path::new(path).parent().unwrap().join(file);
                            if !std::fs::metadata(&shard).is_ok_and(|m| m.is_file() && m.len() > 0) {
                                missing.push(format!("{key} missing shard: {}", shard.display()));
                            }
                        }
                    },
                    _ => missing.push(format!("invalid model index: {path}")),
                }
            }
        }
    }
    match get(doc, "minimax_h3.required_tools").and_then(|v| v.as_array().ok_or("minimax_h3.required_tools must be an array".into())) {
        Ok(tools) => for tool in tools {
            let p = tool.as_str().unwrap_or("");
            if !executable(Path::new(p)) { missing.push(format!("missing executable: {p}")); }
        },
        Err(e) => missing.push(e),
    }
    if let Ok(runner) = string(doc, "minimax_h3.runner") {
        if !executable(Path::new(runner)) { missing.push(format!("runner is not executable: {runner}")); }
    }
    missing
}

pub fn executable(path: &Path) -> bool {
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))] { path.is_file() }
}

pub fn register(doc: &Value) -> Result<(), String> {
    let root = Path::new(string(doc, "server.model_root")?);
    let entries = get(doc, "registry")?.as_array().ok_or("registry must be an array")?;
    // Validate every existing target before making any changes.
    let mut links = Vec::new();
    for entry in entries {
        let name = string(entry, "name")?;
        if Path::new(name).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(format!("registry name must be a relative path without traversal: {name}"));
        }
        let source = string(doc, string(entry, "source_key")?);
        let source = match source.and_then(|s| Path::new(s).canonicalize().map_err(|e| format!("{s}: {e}"))) {
            Ok(p) => p,
            Err(e) if entry["optional"] == true => { eprintln!("Not registered ({name}): {e}"); continue; },
            Err(e) => return Err(e),
        };
        let target = root.join(name);
        if std::fs::symlink_metadata(&target).is_ok() {
            if target.canonicalize().ok().as_ref() == Some(&source) { continue; }
            return Err(format!("refusing to replace registry entry: {}", target.display()));
        }
        links.push((source, target));
    }
    for (source, target) in links {
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        #[cfg(unix)] std::os::unix::fs::symlink(&source, &target).map_err(|e| e.to_string())?;
        #[cfg(not(unix))] return Err("registry symlink installation is not implemented on this platform".into());
        println!("Registered: {} -> {}", target.display(), source.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("difc config test {}-{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            std::fs::create_dir(&p).unwrap(); Self(p)
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

    #[test] fn selected_profile_resolves_from_an_unrelated_directory() {
        let temp = Temp::new();
        let base = repository_root().join("config/difc.json");
        let path = temp.0.join("machine's profile.json");
        std::fs::write(&path, json!({"extends":base, "server":{"port":7899},
            "minimax_h3":{"fixture_dir":"${config}/different artifacts",
                "video_program":"${minimax_h3.fixture_dir}/decoder-native-tile-l36.difir"}}).to_string()).unwrap();
        let doc = load(&path, &repository_root()).unwrap();
        assert_eq!(doc["server"]["port"], 7899);
        assert_eq!(doc["minimax_h3"]["video_program"], temp.0.join("different artifacts/decoder-native-tile-l36.difir").to_str().unwrap());
        assert!(doc["sdxl"]["checkpoint"].as_str().unwrap().starts_with('/'));
    }
    #[test] fn inheritance_cycles_and_invalid_json_fail_before_execution() {
        let temp = Temp::new(); let path = temp.0.join("cycle.json");
        std::fs::write(&path, r#"{"extends":"cycle.json"}"#).unwrap();
        assert!(load(&path, &temp.0).unwrap_err().contains("cycle"));
        std::fs::write(&path, "{bad json}").unwrap();
        assert!(load(&path, &temp.0).is_err());
    }
    #[test] fn h3_task_profiles_resolve_checkpoint_dependent_paths_and_reject_wrong_task() {
        let temp = Temp::new();
        let base_path = repository_root().join("config/difc.json");
        let child = temp.0.join("ref task.json");
        std::fs::write(&child, json!({"extends":base_path,
            "minimax_h3":{"checkpoint":"${config}/reference checkpoint",
                "profile":{"task":"ref2va","tasks":["ref2va"]}}}).to_string()).unwrap();
        let mut base = load(&base_path, &repository_root()).unwrap();
        base["minimax_h3"]["task_configs"]["ref2va"] = json!(child);
        let selected = h3_task_document(&base, "ref2va").unwrap();
        assert_ne!(selected["minimax_h3"]["checkpoint"], base["minimax_h3"]["checkpoint"]);
        assert!(selected["minimax_h3"]["transformer_index"].as_str().unwrap()
            .starts_with(temp.0.join("reference checkpoint").to_str().unwrap()));
        assert_eq!(h3_task_document(&base, "t2va").unwrap(), base);
        let mut request = selected["minimax_h3"]["profile"].clone();
        request["task"] = json!("ref2va");
        assert!(h3_contract(&base, &request).is_ok());
        base["minimax_h3"]["task_configs"]["i2va"] = json!(child);
        assert!(h3_task_document(&base, "i2va").unwrap_err().contains("does not declare"));
    }
    #[test] fn motion_contract_includes_overlap_and_alignment_not_just_delivery() {
        let base = load(&repository_root().join("config/difc.json"), &repository_root()).unwrap();
        let mut request = base["minimax_h3"]["profile"].clone();
        request["task"] = json!("continue");
        for (overlap, internal) in [(5,141), (22,158), (39,175)] {
            request["motion_context_frames"] = json!(overlap);
            request["frames"] = json!(internal);
            assert!(h3_contract(&base, &request).is_ok());
            request["frames"] = json!(124);
            assert!(h3_contract(&base, &request).is_err());
        }
        request["motion_context_frames"] = json!(6);
        assert!(h3_contract(&base, &request).is_err());
    }
    #[test] fn registry_is_idempotent_and_preserves_existing_files() {
        let temp = Temp::new(); let source = temp.0.join("weight's file");
        std::fs::write(&source, b"configuration fixture, not a model").unwrap();
        let root = temp.0.join("registry");
        let mut doc = json!({"server":{"model_root":root}, "model":{"checkpoint":source},
            "registry":[{"name":"checkpoints/custom.bin", "source_key":"model.checkpoint"}]});
        register(&doc).unwrap(); register(&doc).unwrap();
        let other = root.join("checkpoints/user.bin"); std::fs::write(&other, b"preserve me").unwrap();
        doc["registry"][0]["name"] = json!("checkpoints/user.bin");
        assert!(register(&doc).unwrap_err().contains("refusing to replace"));
        assert_eq!(std::fs::read(&other).unwrap(), b"preserve me");
        doc["registry"][0]["name"] = json!("../escape.bin");
        assert!(register(&doc).is_err());
    }
    #[test] fn json_aliases_and_h3_profile_drive_admission() {
        let mut doc = load(&repository_root().join("config/difc.json"), &repository_root()).unwrap();
        doc["image_profiles"]["sdxl"]["aliases"] = json!(["custom-xl"]);
        assert_eq!(image_profile_in(&doc, "custom-xl.safetensors").unwrap().0, "sdxl");
        assert!(image_profile_in(&doc, "sd_xl_base_1.0").is_none());
        let mut request = doc["minimax_h3"]["profile"].clone();
        assert!(h3_contract(&doc, &request).is_ok());
        // Geometry is authored per request: an off-profile frame count is
        // admitted, and only a non-positive one is refused. The runtime
        // geometry check and the runner still verify the 17-frame alignment.
        request["frames"] = json!(73);
        assert!(h3_contract(&doc, &request).is_ok());
        request["frames"] = json!(0);
        assert!(h3_contract(&doc, &request).unwrap_err().contains("frames"));
        request["frames"] = json!(124);
        // FPS stays sealed: output framing and internal alignment derive from it.
        request["fps"] = json!(30);
        assert!(h3_contract(&doc, &request).unwrap_err().contains("fps"));
        request["fps"] = doc["minimax_h3"]["profile"]["fps"].clone();
        assert!(h3_contract(&doc, &request).is_ok());
        doc["minimax_h3"]["profile"]["tasks"] = json!(["t2va"]);
        request["task"] = json!("fl2va"); assert!(h3_contract(&doc, &request).is_err());
        doc["minimax_h3"]["profile"]["tasks"] = json!(["t2va", "fl2va"]);
        assert!(h3_contract(&doc, &request).is_ok());
        // Ref2VA routes to its own task config (config/h3-ref2va.json), which
        // declares tasks:["ref2va"], so it is admitted regardless of the base
        // profile's task list. This previously read as an error only because
        // that document carries no geometry and the sealed comparison failed on
        // a null -- an incidental pass, not the task-gating this test names.
        request["task"] = json!("ref2va"); assert!(h3_contract(&doc, &request).is_ok());
        // A task with no config of its own and not in profile.tasks is refused.
        request["task"] = json!("i2va"); assert!(h3_contract(&doc, &request).is_err());
    }
    #[test] fn h3_step_cache_modes_are_opt_in_with_exact_default() {
        let mut doc = load(&repository_root().join("config/difc.json"), &repository_root()).unwrap();
        let mut request = doc["minimax_h3"]["profile"].clone();
        assert_eq!(request["step_cache"], "exact");
        assert!(h3_contract(&doc, &request).is_ok());
        request["step_cache"] = json!("high");
        assert!(h3_contract(&doc, &request).is_ok());
        request["step_cache"] = json!("easycache");
        assert!(h3_contract(&doc, &request).unwrap_err().contains("step_cache"));
        doc["minimax_h3"]["profile"]["step_cache_modes"] = json!(["exact"]);
        request["step_cache"] = json!("high");
        assert!(h3_contract(&doc, &request).is_err());
        doc["minimax_h3"]["profile"].as_object_mut().unwrap().remove("step_cache_modes");
        assert!(h3_contract(&doc, &request).is_err());
        request["step_cache"] = json!("exact");
        assert!(h3_contract(&doc, &request).is_ok());
    }
    #[test] fn invalid_h3_step_cache_modes_fail_configuration() {
        let mut doc = load(&repository_root().join("config/difc.json"), &repository_root()).unwrap();
        for modes in [json!([]), json!(["high"]), json!(["exact", "typo"]), json!("exact")] {
            doc["minimax_h3"]["profile"]["step_cache_modes"] = modes;
            assert!(validate(&doc).unwrap_err().contains("step_cache_modes"));
        }
    }
    #[test] fn invalid_port_and_duplicate_aliases_are_errors() {
        let mut doc = load(&repository_root().join("config/difc.json"), &repository_root()).unwrap();
        doc["server"]["port"] = json!(65536); assert!(validate(&doc).is_err());
        doc["server"]["port"] = json!(7811);
        doc["image_profiles"]["sdxl"]["aliases"] = json!(["flux-2-klein-base-4b"]);
        assert!(validate(&doc).unwrap_err().contains("duplicate"));
    }
    #[test] fn references_are_literal_and_recursive() {
        let root = json!({"p":"${repo}/models with spaces", "q":"${p}/x'$(false).bin"});
        let out = expand(&root, &root, &json!({"repo":"/example"}), &mut vec![]).unwrap();
        assert_eq!(out["q"], "/example/models with spaces/x'$(false).bin");
    }
    #[test] fn cycles_missing_and_wrong_types_fail() {
        for root in [json!({"a":"${a}"}), json!({"a":"${missing}"}), json!({"a":"${b}","b":4}), json!({"a":"${oops"})] {
            assert!(expand(&root, &root, &json!({}), &mut vec![]).is_err());
        }
    }
    #[test] fn nested_overrides_preserve_objects_replace_arrays() {
        let mut a = json!({"h3":{"x":1,"args":[1,2]},"other":true});
        merge(&mut a, json!({"h3":{"args":[3]}}));
        assert_eq!(a, json!({"h3":{"x":1,"args":[3]},"other":true}));
    }
    #[test] fn malformed_prerequisites_are_not_ready() {
        assert!(!h3_missing(&json!({}), "int8").is_empty());
        assert!(!h3_task_missing(&json!({}), "i2va", "int8").is_empty());
        let doc = json!({"minimax_h3":{"profile":{"tasks":["ref2va"]}}});
        assert!(!h3_task_missing(&doc, "ref2va", "bf16").is_empty());
    }
    #[test] fn ref2va_audio_prerequisites_follow_admitted_reference_kinds() {
        let temp = Temp::new();
        let file = temp.0.join("audio encoder fixture");
        std::fs::write(&file, b"fixture").unwrap();
        let executable = std::env::current_exe().unwrap();
        let mut doc = json!({"minimax_h3":{
            "profile":{"tasks":["ref2va"], "quant_modes":["bf16"]},
            "references":{"kinds":["image"]}
        }});
        let audio_missing = |doc: &Value| h3_task_missing(doc, "ref2va", "bf16").into_iter()
            .filter(|error| error.contains("references.audio_encoder")).collect::<Vec<_>>();
        assert!(audio_missing(&doc).is_empty());
        doc["minimax_h3"]["references"]["kinds"] = json!(["image", "audio"]);
        assert_eq!(audio_missing(&doc).len(), 6);
        doc["minimax_h3"]["references"]["audio_encoder"] = json!({
            "tool":executable, "ffmpeg":executable, "ffprobe":executable,
            "config":file, "checkpoint":file, "modulation_cache":file
        });
        assert!(audio_missing(&doc).is_empty());
        doc["minimax_h3"]["references"]["audio_encoder"]["tool"] = json!(temp.0.join("missing tool"));
        assert!(audio_missing(&doc)[0].contains("tool is not executable"));
        std::fs::write(&file, b"").unwrap();
        assert_eq!(audio_missing(&doc).len(), 4);
    }
}
