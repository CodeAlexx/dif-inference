//! serenity_worker_difc — a serenity-server IPC worker whose generation
//! engine is the Diffusion Compiler's native process chains.
//!
//! Contract (crates/wire, crates/ipc): argv[last] is a connected AF_UNIX
//! SOCK_STREAM fd. Emit `{"ev":"ready"}` at once. Read '\n'-framed JSON
//! commands: `start` (all JobParams keys), `cancel`, `sampling_ack` (ignored),
//! anything unknown ignored. Emit `progress` / `done` / `failed` / `cancelled`.
//!
//! Engine: every family is a chain of compiler processes run in a fresh
//! per-job work directory. Progress is parsed from the tools' stdout step
//! lines; cancel is SIGTERM (then SIGKILL) on the child's process group.
//! Knobs a chain cannot honor fail the job loudly by name; nothing is
//! silently dropped (the wire crate's rule).

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use serenity_wire::JobParams;

// ───────────────────────────── configuration ─────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct Config {
    /// Directory holding the compiler executables (dif*). Used as-is; never rebuilt here.
    compiler_build: String,
    /// flock file serializing GPU work with everything else on the box.
    #[serde(default)]
    gpu_lock: String,
    /// Seconds to wait for the GPU lock before failing the job.
    #[serde(default)]
    gpu_lock_wait_seconds: u64,
    /// Host-memory cap for each compiler stage, applied with a systemd user
    /// scope (stays inside the worker's process group so cancel reaches it).
    /// Same properties as scripts/mem_safe_runtime.sh; empty = no cap.
    #[serde(default)]
    memory_max: String,
    #[serde(default)]
    memory_swap_max: String,
    /// Admission: MemAvailable must cover memory_max + this reserve before a stage starts.
    #[serde(default)]
    desktop_reserve: String,
    /// Extra environment for every stage.
    #[serde(default)]
    stage_env: BTreeMap<String, String>,
    #[serde(default)]
    flux2_klein_base_9b: Option<Flux2Config>,
    #[serde(default)]
    flux2_klein_base_4b: Option<Flux2Config>,
    #[serde(default)]
    sdxl: Option<SdxlConfig>,
    /// Krea 2 chain settings are read by scripts/krea2_chain.sh; the worker only
    /// checks the section exists and that the checkpoints are on disk.
    #[serde(default)]
    krea2: Option<Krea2Config>,
}

#[derive(Debug, Clone, Deserialize)]
struct Krea2Config {
    runner: String,
    turbo_checkpoint: String,
    raw_checkpoint: String,
    conditioner_bundle: String,
    vae_checkpoint: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Flux2Config {
    model_dir: String,
    #[serde(default)]
    text_encoder_dir: Option<String>,
    #[serde(default)]
    tokenizer_dir: Option<String>,
    transformer_checkpoint: String,
    vae_checkpoint: String,
    cache_dir: String,
    /// Accepted execution-policy flags (precision route, residency, staging).
    #[serde(default)]
    extra_args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SdxlConfig {
    checkpoint: String,
    tokenizer_dir: String,
    cache_dir: String,
    #[serde(default)]
    extra_args: Vec<String>,
}

fn repository_root() -> PathBuf {
    if let Some(root) = std::env::var_os("SERENITY_REPO_ROOT") {
        return PathBuf::from(root);
    }
    // output/bin/serenity_worker_difc -> repository root two levels up.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.ancestors().nth(3) {
            return root.to_path_buf();
        }
    }
    PathBuf::from(".")
}

fn load_config() -> Result<Config, String> {
    let path = difc_config::config_path();
    let doc = difc_config::load(&path, &repository_root())?;
    let cfg: Config = serde_json::from_value(doc)
        .map_err(|e| format!("invalid worker config {}: {e}", path.display()))?;
    if !Path::new(&cfg.compiler_build).is_dir() {
        return Err(format!(
            "compiler build directory does not exist: {}",
            cfg.compiler_build
        ));
    }
    Ok(cfg)
}

// ───────────────────────────── IPC framing ─────────────────────────────

struct Wire {
    out: Mutex<UnixStream>,
}

impl Wire {
    fn send(&self, v: Value) {
        let mut line = v.to_string();
        line.push('\n');
        let mut s = self.out.lock().unwrap();
        // MSG_NOSIGNAL semantics: ignore SIGPIPE at process level (set in main).
        let _ = s.write_all(line.as_bytes());
        let _ = s.flush();
    }
    fn ready(&self) {
        self.send(json!({"ev": "ready"}));
    }
    fn progress(&self, step: i64, total: i64, phase: &str) {
        self.send(json!({"ev": "progress", "step": step, "total": total, "phase": phase, "preview": ""}));
    }
    fn done(&self, output_path: &Path) {
        self.send(json!({"ev": "done", "output_path": output_path.to_string_lossy()}));
    }
    fn failed(&self, error: &str) {
        eprintln!("[difc-worker] failed: {error}");
        self.send(json!({"ev": "failed", "error": error}));
    }
    fn cancelled(&self) {
        self.send(json!({"ev": "cancelled"}));
    }
}

enum Msg {
    Command(Value),
    PeerClosed,
    ChildLine(String),
    ChildExit,
}

fn spawn_command_reader(stream: UnixStream, tx: Sender<Msg>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(Msg::PeerClosed);
                    return;
                }
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\n', '\r']);
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(trimmed) {
                        Ok(v) => {
                            if tx.send(Msg::Command(v)).is_err() {
                                return;
                            }
                        }
                        Err(e) => eprintln!("[difc-worker] ignoring unparseable command line: {e}"),
                    }
                }
            }
        }
    });
}

// ───────────────────────────── PNG helpers ─────────────────────────────

mod png {
    use std::sync::OnceLock;

    fn crc_table() -> &'static [u32; 256] {
        static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
        TABLE.get_or_init(|| {
            let mut t = [0u32; 256];
            for (n, slot) in t.iter_mut().enumerate() {
                let mut c = n as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
                *slot = c;
            }
            t
        })
    }

    pub fn crc32(bytes: &[u8]) -> u32 {
        let t = crc_table();
        let mut c = 0xFFFF_FFFFu32;
        for &b in bytes {
            c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        c ^ 0xFFFF_FFFF
    }

    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = Vec::with_capacity(4 + data.len());
        body.extend_from_slice(kind);
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }

    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    /// Insert a tEXt chunk right after IHDR. Returns the new file bytes.
    pub fn with_text_chunk(file: &[u8], keyword: &str, text: &str) -> Result<Vec<u8>, String> {
        if file.len() < 33 || file[..8] != SIGNATURE {
            return Err("not a PNG file".into());
        }
        let ihdr_len = u32::from_be_bytes([file[8], file[9], file[10], file[11]]) as usize;
        let ihdr_end = 8 + 4 + 4 + ihdr_len + 4;
        if &file[12..16] != b"IHDR" || file.len() < ihdr_end {
            return Err("PNG without a leading IHDR chunk".into());
        }
        let mut data = Vec::with_capacity(keyword.len() + 1 + text.len());
        data.extend_from_slice(keyword.as_bytes());
        data.push(0);
        data.extend_from_slice(text.as_bytes());
        let mut out = Vec::with_capacity(file.len() + data.len() + 12);
        out.extend_from_slice(&file[..ihdr_end]);
        chunk(&mut out, b"tEXt", &data);
        out.extend_from_slice(&file[ihdr_end..]);
        Ok(out)
    }

    fn adler32(bytes: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in bytes {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    /// Minimal RGB8 PNG writer using stored (uncompressed) deflate blocks.
    pub fn encode_rgb8(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
        let row = width as usize * 3;
        let mut raw = Vec::with_capacity((row + 1) * height as usize);
        for y in 0..height as usize {
            raw.push(0); // filter: none
            raw.extend_from_slice(&rgb[y * row..(y + 1) * row]);
        }
        let mut z = vec![0x78, 0x01];
        let mut pos = 0;
        while pos < raw.len() || raw.is_empty() {
            let n = (raw.len() - pos).min(65535);
            let last = pos + n >= raw.len();
            z.push(if last { 1 } else { 0 });
            z.extend_from_slice(&(n as u16).to_le_bytes());
            z.extend_from_slice(&(!(n as u16)).to_le_bytes());
            z.extend_from_slice(&raw[pos..pos + n]);
            pos += n;
            if raw.is_empty() {
                break;
            }
        }
        z.extend_from_slice(&adler32(&raw).to_be_bytes());
        let mut out = SIGNATURE.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &z);
        chunk(&mut out, b"IEND", &[]);
        out
    }
}

// ───────────────────────────── families / chains ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Stub,
    Flux2KleinBase9b,
    Flux2KleinBase4b,
    SdxlBase,
    Krea2Turbo,
    Krea2Raw,
}

impl Family {
    fn key(self) -> &'static str {
        match self {
            Family::Stub => "stub",
            Family::Flux2KleinBase9b => "flux2_klein_base_9b",
            Family::Flux2KleinBase4b => "flux2_klein_base_4b",
            Family::SdxlBase => "sdxl",
            Family::Krea2Turbo => "krea2_turbo",
            Family::Krea2Raw => "krea2_raw",
        }
    }
}

fn resolve_family(model: &str, checkpoint_path: &str) -> Result<Family, String> {
    let m = model.to_ascii_lowercase();
    let c = checkpoint_path.to_ascii_lowercase();
    if m == "stub" {
        return Ok(Family::Stub);
    }
    let klein = |s: &str| {
        // JSON owns names/checkpoints; these keys select implemented algorithms.
        match difc_config::image_profile(s).map(|(key, _)| key) {
            Some("flux2_klein_base_9b") => Some(Family::Flux2KleinBase9b),
            Some("flux2_klein_base_4b") => Some(Family::Flux2KleinBase4b),
            Some("sdxl") => Some(Family::SdxlBase),
            _ => None,
        }
    };
    let from_model = klein(&m);
    let from_checkpoint = klein(&c);
    if let (Some(a), Some(b)) = (from_model, from_checkpoint) {
        if a != b { return Err("selected model and checkpoint identify different compiler profiles".into()); }
    }
    if let Some(family) = from_model.or(from_checkpoint) {
        return Ok(family);
    }
    if m.contains("krea") || c.contains("krea") {
        return Ok(if m.contains("raw") || c.contains("raw") { Family::Krea2Raw } else { Family::Krea2Turbo });
    }
    Err(format!(
        "model '{model}' (checkpoint '{checkpoint_path}') is not a family the Diffusion Compiler serves here; \
         admitted: FLUX.2 [klein] Base 9B / Base 4B, SDXL Base 1.0, Krea 2 Turbo / Raw, stub"
    ))
}

/// Knobs no compiler chain honors. Listed by name so the refusal is exact.
fn unsupported_knobs(p: &JobParams, negative_supported: bool) -> Vec<String> {
    let mut u = Vec::new();
    if !negative_supported && !p.negative.trim().is_empty() {
        u.push("negative prompt (chain has no negative text conditioning; creator uses an empty negative)".into());
    }
    if !p.loras.is_empty() {
        u.push(format!("lora ({} row(s); no LoRA route in this chain)", p.loras.len()));
    }
    for (name, v) in [
        ("init_image", &p.init_image),
        ("mask_image", &p.mask_image),
        ("edit_src_image", &p.edit_src_image),
        ("reference_image", &p.reference_image),
        ("inpaint_conditioning_image", &p.inpaint_conditioning_image),
        ("qwen_edit_conditioning_image", &p.qwen_edit_conditioning_image),
        ("vae", &p.vae),
    ] {
        if !v.is_empty() {
            u.push(format!("{name}={v}"));
        }
    }
    if p.clip_skip != 0 {
        u.push(format!("clip_skip={}", p.clip_skip));
    }
    if p.eta != -1.0 {
        u.push(format!("eta={}", p.eta));
    }
    if p.sigma_min != -1.0 {
        u.push(format!("sigma_min={}", p.sigma_min));
    }
    if p.sigma_max != -1.0 {
        u.push(format!("sigma_max={}", p.sigma_max));
    }
    if p.restart_sampling {
        u.push("restart_sampling=true".into());
    }
    if p.variation_strength != 0.0 {
        u.push(format!("variation_strength={}", p.variation_strength));
    }
    if p.cfg_override != -1.0 {
        u.push(format!("cfg_override={}", p.cfg_override));
    }
    u
}

struct Chain {
    family: Family,
    /// argv of the single heavy stage (P1: one process). Multi-stage chains come with H3.
    argv: Vec<String>,
    /// stdout prefix whose `step=N/M` tail carries progress.
    step_prefix: &'static str,
    total_steps: i64,
    /// where the stage writes the image, and its JSON report
    image: PathBuf,
    report: PathBuf,
}

fn build_flux2_chain(cfg: &Config, p: &JobParams, work: &Path, family: Family) -> Result<Chain, String> {
    let (settings, variant) = match family {
        Family::Flux2KleinBase9b => (&cfg.flux2_klein_base_9b, "klein9b"),
        Family::Flux2KleinBase4b => (&cfg.flux2_klein_base_4b, "klein4b"),
        _ => return Err("not a Klein compiler profile".into()),
    };
    let f = settings.as_ref().ok_or_else(|| format!("worker config has no {} section", family.key()))?;
    for (what, path) in [
        ("model_dir", &f.model_dir),
        ("transformer_checkpoint", &f.transformer_checkpoint),
        ("vae_checkpoint", &f.vae_checkpoint),
    ] {
        if !Path::new(path).exists() {
            return Err(format!("flux2 {what} missing on disk: {path}"));
        }
    }
    if !p.checkpoint_path.is_empty() {
        let selected = fs::canonicalize(&p.checkpoint_path)
            .map_err(|e| format!("cannot resolve selected checkpoint: {e}"))?;
        let configured = fs::canonicalize(&f.transformer_checkpoint)
            .map_err(|e| format!("cannot resolve configured checkpoint: {e}"))?;
        if selected != configured {
            return Err("selected checkpoint is not the configured Klein profile; refusing to substitute weights".into());
        }
    }
    for (what, path) in [
        ("text encoder", f.text_encoder_dir.as_ref()),
        ("tokenizer", f.tokenizer_dir.as_ref()),
    ] {
        if let Some(path) = path {
            if !Path::new(path).is_dir() { return Err(format!("flux2 {what} directory missing: {path}")); }
        }
    }
    let sampler = p.sampler.to_ascii_lowercase();
    if !(sampler.is_empty() || sampler == "euler") {
        return Err(format!(
            "sampler '{}' unsupported: the FLUX.2 klein chain runs the creator generalized-time Euler sampler only",
            p.sampler
        ));
    }
    let scheduler = p.scheduler.to_ascii_lowercase();
    if !(scheduler.is_empty() || scheduler == "simple" || scheduler == "flux2") {
        return Err(format!(
            "scheduler '{}' unsupported: the FLUX.2 klein chain uses the creator flux2 schedule only",
            p.scheduler
        ));
    }
    let u = unsupported_knobs(p, false);
    if !u.is_empty() {
        return Err(format!(
            "request carries knobs the FLUX.2 klein compiler chain cannot honor (refused, not dropped): {}",
            u.join("; ")
        ));
    }
    if p.width <= 0 || p.height <= 0 || p.width % 16 != 0 || p.height % 16 != 0 {
        return Err(format!(
            "width x height {}x{} must be positive multiples of 16 (VAE 8 x patch 2)",
            p.width, p.height
        ));
    }
    if !p.cfg.is_finite() || p.cfg < 0.0 {
        return Err("CFG must be finite and nonnegative".into());
    }
    if p.steps < 1 {
        return Err(format!("steps={} must be >= 1", p.steps));
    }
    if p.prompt.trim().is_empty() {
        return Err("empty prompt".into());
    }
    let seed = p.seed.unsigned_abs();
    let image = work.join("image.png");
    let report = work.join("report.json");
    let mut argv = vec![
        format!("{}/difflux2sample", cfg.compiler_build),
        "--flux2-model".into(),
        variant.into(),
        "--model-dir".into(),
        f.model_dir.clone(),
        "--transformer-checkpoint".into(),
        f.transformer_checkpoint.clone(),
        "--vae-checkpoint".into(),
        f.vae_checkpoint.clone(),
        "--prompt".into(),
        p.prompt.clone(),
        "--seed".into(),
        seed.to_string(),
        "--steps".into(),
        p.steps.to_string(),
        "--width".into(),
        p.width.to_string(),
        "--height".into(),
        p.height.to_string(),
        "--guidance".into(),
        format!("{}", p.cfg),
        "--cache-dir".into(),
        f.cache_dir.clone(),
        "--output".into(),
        image.to_string_lossy().into_owned(),
        "--report".into(),
        report.to_string_lossy().into_owned(),
    ];
    if let Some(path) = &f.text_encoder_dir {
        argv.extend(["--text-encoder-dir".into(), path.clone()]);
    }
    if let Some(path) = &f.tokenizer_dir {
        argv.extend(["--tokenizer-dir".into(), path.clone()]);
    }
    // Execution policy must not replace semantic request/model arguments.
    for arg in &f.extra_args {
        if ["--flux2-model", "--model-dir", "--transformer-checkpoint", "--vae-checkpoint",
            "--text-encoder-dir", "--tokenizer-dir", "--prompt", "--seed", "--steps", "--width",
            "--height", "--guidance", "--output", "--report", "--cache-dir"]
            .contains(&arg.as_str()) {
            return Err(format!("{} extra_args may not override {arg}", family.key()));
        }
    }
    argv.extend(f.extra_args.iter().cloned());
    Ok(Chain {
        family,
        argv,
        step_prefix: "FLUX2_NATIVE_STEP",
        total_steps: p.steps,
        image,
        report,
    })
}

fn build_sdxl_chain(cfg: &Config, p: &JobParams, work: &Path) -> Result<Chain, String> {
    let f = cfg.sdxl.as_ref().ok_or("worker config has no sdxl section")?;
    for path in [PathBuf::from(&f.checkpoint), Path::new(&f.tokenizer_dir).join("vocab.json"),
                 Path::new(&f.tokenizer_dir).join("merges.txt")] {
        if !path.is_file() { return Err(format!("SDXL artifact missing: {}", path.display())); }
    }
    if !p.checkpoint_path.is_empty() &&
        fs::canonicalize(&p.checkpoint_path).map_err(|e| format!("selected checkpoint: {e}"))? !=
        fs::canonicalize(&f.checkpoint).map_err(|e| format!("configured checkpoint: {e}"))? {
        return Err("selected checkpoint is not the configured SDXL profile; refusing to substitute weights".into());
    }
    if !matches!(p.sampler.to_ascii_lowercase().as_str(), "" | "euler") ||
        !matches!(p.scheduler.to_ascii_lowercase().as_str(), "" | "normal") {
        return Err("SDXL compiler chain supports Euler with the normal discrete DDPM schedule only".into());
    }
    let unsupported = unsupported_knobs(p, true);
    if !unsupported.is_empty() {
        return Err(format!("SDXL compiler chain cannot honor: {}", unsupported.join("; ")));
    }
    if p.width <= 0 || p.height <= 0 || p.width % 8 != 0 || p.height % 8 != 0 {
        return Err("SDXL width and height must be positive multiples of eight".into());
    }
    if p.steps < 1 || p.steps > i64::from(u32::MAX) || !p.cfg.is_finite() || p.cfg < 0.0 {
        return Err("SDXL requires positive uint32 steps and finite nonnegative CFG".into());
    }
    if p.prompt.trim().is_empty() { return Err("empty prompt".into()); }
    // Dtype is an execution policy. No extra flag may replace user semantics,
    // change the output path, or quietly produce a different number of images.
    if f.extra_args.len() % 2 != 0 { return Err("sdxl extra_args must be dtype flag/value pairs".into()); }
    for pair in f.extra_args.chunks_exact(2) {
        let accepted = match pair[0].as_str() {
            "--unet-dtype" | "--clip-dtype" => matches!(pair[1].as_str(), "f16" | "bf16" | "f32"),
            "--vae-dtype" => matches!(pair[1].as_str(), "bf16" | "f32"),
            _ => false,
        };
        if !accepted { return Err(format!("sdxl extra_args refuses {} {}", pair[0], pair[1])); }
    }
    let image = work.join("image.png");
    let report = work.join("report.json");
    let mut argv = vec![format!("{}/difsdxlsample", cfg.compiler_build),
        "--checkpoint".into(), f.checkpoint.clone(), "--tokenizer-dir".into(), f.tokenizer_dir.clone(),
        "--prompt".into(), p.prompt.clone(), "--negative".into(), p.negative.clone(),
        "--seed".into(), p.seed.unsigned_abs().to_string(), "--steps".into(), p.steps.to_string(),
        "--cfg".into(), p.cfg.to_string(), "--width".into(), p.width.to_string(),
        "--height".into(), p.height.to_string(), "--cache-dir".into(), f.cache_dir.clone(),
        "--output".into(), image.to_string_lossy().into_owned(),
        "--report".into(), report.to_string_lossy().into_owned(), "--report-steps".into()];
    argv.extend(f.extra_args.iter().cloned());
    Ok(Chain { family: Family::SdxlBase, argv, step_prefix: "SDXL_NATIVE_STEP", total_steps: p.steps, image, report })
}

fn build_krea2_chain(cfg: &Config, p: &JobParams, work: &Path, family: Family) -> Result<Chain, String> {
    let k = cfg.krea2.as_ref().ok_or("worker config has no krea2 section")?;
    let ckpt = if family == Family::Krea2Raw { &k.raw_checkpoint } else { &k.turbo_checkpoint };
    for (what, path) in [("checkpoint", ckpt), ("conditioner_bundle", &k.conditioner_bundle), ("vae_checkpoint", &k.vae_checkpoint)] {
        if !Path::new(path).exists() {
            return Err(format!("krea2 {what} missing on disk: {path}"));
        }
    }
    let sampler = p.sampler.to_ascii_lowercase();
    if !(sampler.is_empty() || sampler == "euler") {
        return Err(format!("sampler '{}' unsupported: the Krea 2 chain runs the creator Euler sampler only", p.sampler));
    }
    let scheduler = p.scheduler.to_ascii_lowercase();
    if !(scheduler.is_empty() || scheduler == "simple") {
        return Err(format!("scheduler '{}' unsupported: the Krea 2 chain uses the creator shifted flow schedule only", p.scheduler));
    }
    // Turbo is CFG-free (guidance 0): a negative prompt cannot be honored.
    let negative_supported = family == Family::Krea2Raw;
    let u = unsupported_knobs(p, negative_supported);
    if !u.is_empty() {
        return Err(format!("request carries knobs the Krea 2 compiler chain cannot honor (refused, not dropped): {}", u.join("; ")));
    }
    // Geometry is authored, not pinned. This refused anything but 1024x1024,
    // which is a compiled-preset convenience rather than a model limit -- the
    // SDXL chain directly above accepts any positive multiple of eight, and the
    // server already advertises Krea as shape_dispatch across 768..1344. The
    // latent grid is what actually has to divide, so require the same multiple
    // of eight and let the chain build for the requested shape.
    if p.width <= 0 || p.height <= 0 || p.width % 8 != 0 || p.height % 8 != 0 {
        return Err("Krea 2 width and height must be positive multiples of eight".into());
    }
    if p.steps < 1 {
        return Err(format!("steps={} must be >= 1", p.steps));
    }
    if family == Family::Krea2Turbo && p.cfg != 0.0 {
        return Err(format!("cfg={} unsupported: Krea 2 Turbo is guidance-distilled (CFG 0)", p.cfg));
    }
    if p.prompt.trim().is_empty() {
        return Err("empty prompt".into());
    }
    let prompt_file = work.join("prompt.txt");
    fs::write(&prompt_file, &p.prompt).map_err(|e| format!("cannot write prompt: {e}"))?;
    let mut negative_file = String::new();
    if negative_supported && p.cfg > 0.0 {
        let path = work.join("negative.txt");
        fs::write(&path, &p.negative).map_err(|e| format!("cannot write negative: {e}"))?;
        negative_file = path.to_string_lossy().into_owned();
    }
    let script = PathBuf::from(&k.runner);
    let argv = vec![
        script.to_string_lossy().into_owned(),
        work.to_string_lossy().into_owned(),
        prompt_file.to_string_lossy().into_owned(),
        p.seed.unsigned_abs().to_string(),
        p.steps.to_string(),
        format!("{}", p.cfg),
        if family == Family::Krea2Raw { "raw".into() } else { "turbo".into() },
        negative_file,
        // Positional 8 is the diagnostic stop-after; empty in production.
        String::new(),
        // Authored geometry, positional 9 and 10. Without these the chain fell
        // back to its 1024x1024 default and a requested size was silently
        // ignored rather than rendered.
        p.width.to_string(),
        p.height.to_string(),
    ];
    Ok(Chain {
        family,
        argv,
        step_prefix: "KREA2_NATIVE_STEP",
        total_steps: p.steps,
        image: work.join("image.png"),
        report: work.join("sampler-report.json"),
    })
}

// ───────────────────────────── process control ─────────────────────────────

struct Stage {
    child: Child,
    pgid: i32,
}

/// "24G" / "512M" / "1073741824" -> bytes (IEC, like numfmt --from=iec).
fn parse_iec(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, mult) = match s.chars().last()? {
        'K' | 'k' => (&s[..s.len() - 1], 1u64 << 10),
        'M' | 'm' => (&s[..s.len() - 1], 1u64 << 20),
        'G' | 'g' => (&s[..s.len() - 1], 1u64 << 30),
        'T' | 't' => (&s[..s.len() - 1], 1u64 << 40),
        _ => (s, 1u64),
    };
    num.parse::<f64>().ok().map(|n| (n * mult as f64) as u64)
}

fn mem_available_bytes() -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// The mem_safe_runtime.sh admission rule: refuse to start a stage unless the
/// host can hold the stage's cap plus the desktop reserve.
fn admit_host_memory(cfg: &Config) -> Result<(), String> {
    let max = parse_iec(&cfg.memory_max);
    let reserve = parse_iec(&cfg.desktop_reserve).unwrap_or(0);
    if let (Some(max), Some(avail)) = (max, mem_available_bytes()) {
        if avail < max + reserve {
            return Err(format!(
                "host memory admission refused: MemAvailable {:.1} GiB < cap {} + reserve {}",
                avail as f64 / (1u64 << 30) as f64,
                cfg.memory_max,
                cfg.desktop_reserve
            ));
        }
    }
    Ok(())
}

fn spawn_stage(cfg: &Config, argv: &[String], stderr_log: &Path, tx: Sender<Msg>) -> Result<Stage, String> {
    admit_host_memory(cfg)?;
    // flock serializes the GPU; a systemd user scope caps host memory while
    // keeping the tool in this process group (a transient *service* would not).
    let mut full: Vec<String> = vec![
        "flock".into(),
        "-w".into(),
        cfg.gpu_lock_wait_seconds.to_string(),
        cfg.gpu_lock.clone(),
    ];
    if !cfg.memory_max.is_empty() {
        full.extend(["systemd-run", "--user", "--scope", "--quiet", "-p", "MemoryHigh=infinity"].map(String::from));
        full.push("-p".into());
        full.push(format!("MemoryMax={}", cfg.memory_max));
        if !cfg.memory_swap_max.is_empty() {
            full.push("-p".into());
            full.push(format!("MemorySwapMax={}", cfg.memory_swap_max));
        }
        full.push("--".into());
    }
    full.extend(argv.iter().cloned());
    let err_file = fs::File::create(stderr_log).map_err(|e| format!("cannot create {}: {e}", stderr_log.display()))?;
    let mut cmd = Command::new(&full[0]);
    cmd.args(&full[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(err_file);
    for (k, v) in &cfg.stage_env {
        cmd.env(k, v);
    }
    // The stage runs under this worker's flock; chain scripts must not re-lock.
    cmd.env("DIFC_LOCK_HELD", "1");
    cmd.env("SERENITY_REPO_ROOT", repository_root());
    // Own process group so cancel reaches flock, the runner, and the tool.
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot spawn {}: {e}", full[0]))?;
    let pgid = child.id() as i32;
    let stdout = child.stdout.take().expect("piped stdout");
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(Msg::ChildLine(line.trim_end().to_string())).is_err() {
                        break;
                    }
                }
            }
        }
        let _ = tx.send(Msg::ChildExit);
    });
    Ok(Stage { child, pgid })
}

fn terminate_group(stage: &mut Stage) {
    unsafe {
        libc::kill(-stage.pgid, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(Some(_)) = stage.child.try_wait() {
            break;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-stage.pgid, libc::SIGKILL);
            }
            let _ = stage.child.wait();
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn parse_step(line: &str, prefix: &str) -> Option<(i64, i64)> {
    if !line.starts_with(prefix) {
        return None;
    }
    let tail = line.split_whitespace().find_map(|tok| tok.strip_prefix("step="))?;
    let (a, b) = tail.split_once('/')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

enum Outcome {
    Finished(Option<i32>),
    Cancelled,
    PeerClosed,
}

/// Drive one stage to completion while honoring cancel from the control plane.
fn run_stage(
    wire: &Wire,
    rx: &Receiver<Msg>,
    stage: &mut Stage,
    step_prefix: &str,
    total: i64,
    stdout_log: &mut fs::File,
) -> Outcome {
    let mut exit_code: Option<Option<i32>> = None;
    loop {
        match rx.recv() {
            Ok(Msg::ChildLine(line)) => {
                let _ = writeln!(stdout_log, "{line}");
                if let Some((step, _tool_total)) = parse_step(&line, step_prefix) {
                    wire.progress(step.min(total), total, "sampling");
                }
            }
            Ok(Msg::ChildExit) => {
                let status = stage.child.wait().ok();
                exit_code = Some(status.and_then(|s| s.code()));
            }
            Ok(Msg::Command(cmd)) => match cmd.get("cmd").and_then(Value::as_str) {
                Some("cancel") => {
                    terminate_group(stage);
                    return Outcome::Cancelled;
                }
                Some("start") => {
                    eprintln!("[difc-worker] start received while a job is running; ignored");
                }
                _ => {}
            },
            Ok(Msg::PeerClosed) | Err(_) => {
                terminate_group(stage);
                return Outcome::PeerClosed;
            }
        }
        if let Some(code) = exit_code {
            return Outcome::Finished(code);
        }
    }
}

// ───────────────────────────── job driver ─────────────────────────────

fn finalize_png(src: &Path, dst: &Path, params_json: &str) -> Result<(), String> {
    let bytes = fs::read(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    let out = if params_json.is_empty() {
        bytes
    } else {
        png::with_text_chunk(&bytes, "serenity.genparams.v1", params_json)?
    };
    fs::write(dst, out).map_err(|e| format!("cannot write {}: {e}", dst.display()))
}

fn write_result_sidecar(
    output: &Path,
    family: Family,
    argv: &[String],
    report: &Path,
    wall_ms: f64,
    logs: (&Path, &Path),
) {
    let compiler_report = fs::read_to_string(report)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    let doc = json!({
        "schema": "difc.daemon_result.v1",
        "engine": "diffusion-compiler",
        "family": family.key(),
        "argv": argv,
        "wall_ms": wall_ms,
        "stdout_log": logs.0.to_string_lossy(),
        "stderr_log": logs.1.to_string_lossy(),
        "compiler_report": compiler_report,
    });
    let path = PathBuf::from(format!("{}.difc_daemon_result.json", output.display()));
    if let Ok(text) = serde_json::to_string_pretty(&doc) {
        let _ = fs::write(path, text);
    }
}

fn run_stub(wire: &Wire, rx: &Receiver<Msg>, p: &JobParams, output: &Path) -> Result<bool, String> {
    let total = p.steps.max(1);
    for step in 1..=total {
        // Cancel must interrupt the stub the way it interrupts a real stage.
        if let Ok(Msg::Command(cmd)) = rx.recv_timeout(Duration::from_millis(100)) {
            if cmd.get("cmd").and_then(Value::as_str) == Some("cancel") {
                return Ok(false);
            }
        }
        wire.progress(step, total, "sampling");
    }
    let (w, h) = (p.width.max(1) as u32, p.height.max(1) as u32);
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 3) as usize;
            rgb[i] = (x * 255 / w.max(1)) as u8;
            rgb[i + 1] = (y * 255 / h.max(1)) as u8;
            rgb[i + 2] = (p.seed & 0xFF) as u8;
        }
    }
    let bytes = png::encode_rgb8(w, h, &rgb);
    let bytes = if p.params_json.is_empty() {
        bytes
    } else {
        png::with_text_chunk(&bytes, "serenity.genparams.v1", &p.params_json)?
    };
    fs::write(output, bytes).map_err(|e| format!("cannot write {}: {e}", output.display()))?;
    Ok(true)
}

/// Returns false when the peer closed (worker should exit).
fn handle_start(cfg: &Config, wire: &Wire, rx: &Receiver<Msg>, tx: &Sender<Msg>, cmd: Value) -> bool {
    let p: JobParams = match serde_json::from_value(cmd) {
        Ok(p) => p,
        Err(e) => {
            wire.failed(&format!("start command does not decode as JobParams: {e}"));
            return true;
        }
    };
    let job_id = if p.job_id.is_empty() { "job".to_string() } else { p.job_id.clone() };
    let out_dir = PathBuf::from(if p.out_dir.is_empty() { "." } else { &p.out_dir });
    if let Err(e) = fs::create_dir_all(&out_dir) {
        wire.failed(&format!("cannot create out_dir {}: {e}", out_dir.display()));
        return true;
    }
    let output = out_dir.join(format!("{job_id}.png"));
    let family = match resolve_family(&p.model, &p.checkpoint_path) {
        Ok(f) => f,
        Err(e) => {
            wire.failed(&e);
            return true;
        }
    };
    eprintln!("[difc-worker] job {job_id}: family={} model={} steps={} {}x{} seed={}",
        family.key(), p.model, p.steps, p.width, p.height, p.seed);

    if family == Family::Stub {
        return match run_stub(wire, rx, &p, &output) {
            Ok(true) => {
                wire.done(&output);
                true
            }
            Ok(false) => {
                wire.cancelled();
                true
            }
            Err(e) => {
                wire.failed(&e);
                true
            }
        };
    }

    // Fresh work directory per job: the compiler tools refuse to overwrite artifacts.
    let work = out_dir.join("difc").join(&job_id);
    if work.exists() {
        let _ = fs::remove_dir_all(&work);
    }
    if let Err(e) = fs::create_dir_all(&work) {
        wire.failed(&format!("cannot create work dir {}: {e}", work.display()));
        return true;
    }
    let chain = match family {
        Family::Flux2KleinBase9b | Family::Flux2KleinBase4b => build_flux2_chain(cfg, &p, &work, family),
        Family::SdxlBase => build_sdxl_chain(cfg, &p, &work),
        Family::Krea2Turbo | Family::Krea2Raw => build_krea2_chain(cfg, &p, &work, family),
        Family::Stub => unreachable!(),
    };
    let chain = match chain {
        Ok(c) => c,
        Err(e) => {
            wire.failed(&e);
            return true;
        }
    };
    let stdout_log_path = work.join("stdout.log");
    let stderr_log_path = work.join("stderr.log");
    let mut stdout_log = match fs::File::create(&stdout_log_path) {
        Ok(f) => f,
        Err(e) => {
            wire.failed(&format!("cannot create {}: {e}", stdout_log_path.display()));
            return true;
        }
    };
    let _ = fs::write(work.join("argv.json"), serde_json::to_string_pretty(&chain.argv).unwrap_or_default());
    wire.progress(0, chain.total_steps, "preparing");
    let started = Instant::now();
    let mut stage = match spawn_stage(cfg, &chain.argv, &stderr_log_path, tx.clone()) {
        Ok(s) => s,
        Err(e) => {
            wire.failed(&e);
            return true;
        }
    };
    match run_stage(wire, rx, &mut stage, chain.step_prefix, chain.total_steps, &mut stdout_log) {
        Outcome::Cancelled => {
            let _ = fs::remove_file(&chain.image);
            wire.cancelled();
            true
        }
        Outcome::PeerClosed => false,
        Outcome::Finished(code) => {
            let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
            if code != Some(0) {
                let tail = fs::read_to_string(&stderr_log_path)
                    .unwrap_or_default()
                    .lines()
                    .rev()
                    .take(6)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join(" | ");
                wire.failed(&format!(
                    "{} exited with status {:?} after {:.1}s; stderr tail: {tail}",
                    chain.argv[0], code, wall_ms / 1000.0
                ));
                return true;
            }
            if !chain.image.exists() {
                wire.failed(&format!("stage exited 0 but produced no image at {}", chain.image.display()));
                return true;
            }
            if let Err(e) = finalize_png(&chain.image, &output, &p.params_json) {
                wire.failed(&e);
                return true;
            }
            write_result_sidecar(&output, chain.family, &chain.argv, &chain.report, wall_ms, (&stdout_log_path, &stderr_log_path));
            eprintln!("[difc-worker] job {job_id}: done in {:.1}s -> {}", wall_ms / 1000.0, output.display());
            wire.done(&output);
            true
        }
    }
}

fn main() {
    // Peer-close on write must surface as an error, not a SIGPIPE death.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let args: Vec<String> = std::env::args().collect();
    let fd: i32 = match args.last().and_then(|s| s.parse().ok()) {
        Some(fd) if args.len() >= 2 => fd,
        _ => {
            eprintln!("usage: serenity_worker_difc [worker <kind>] <socket_fd>");
            std::process::exit(64);
        }
    };
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    let _ = stream.set_nonblocking(false);
    let reader = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[difc-worker] cannot clone socket: {e}");
            std::process::exit(70);
        }
    };
    let wire = Arc::new(Wire { out: Mutex::new(stream) });
    let (tx, rx) = channel::<Msg>();
    spawn_command_reader(reader, tx.clone());

    // Ready first (15 s server deadline), then validate configuration so a bad
    // config fails the first job loudly instead of the handshake silently.
    wire.ready();
    let cfg = load_config();
    if let Err(e) = &cfg {
        eprintln!("[difc-worker] configuration error: {e}");
    }

    loop {
        match rx.recv() {
            Ok(Msg::Command(cmd)) => match cmd.get("cmd").and_then(Value::as_str) {
                Some("start") => {
                    let cfg = match &cfg {
                        Ok(c) => c,
                        Err(e) => {
                            wire.failed(&format!("worker configuration error: {e}"));
                            continue;
                        }
                    };
                    if !handle_start(cfg, &wire, &rx, &tx, cmd) {
                        return;
                    }
                }
                Some("cancel") => {} // nothing in flight
                Some("sampling_ack") => {}
                other => eprintln!("[difc-worker] ignoring command {other:?}"),
            },
            Ok(Msg::ChildLine(_)) | Ok(Msg::ChildExit) => {} // late lines from a finished stage
            Ok(Msg::PeerClosed) | Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_lines_parse() {
        assert_eq!(parse_step("FLUX2_NATIVE_STEP step=3/50 cfg_batch_ms=1.0", "FLUX2_NATIVE_STEP"), Some((3, 50)));
        assert_eq!(parse_step("KREA2_NATIVE_STEP step=8/8 x", "FLUX2_NATIVE_STEP"), None);
        assert_eq!(parse_step("noise", "FLUX2_NATIVE_STEP"), None);
        assert_eq!(parse_step("SDXL_NATIVE_STEP step=25/25 ms=7.2", "SDXL_NATIVE_STEP"), Some((25, 25)));
        assert_eq!(parse_step("SDXL_STEP 0 sigma=1", "SDXL_NATIVE_STEP"), None);
    }

    #[test]
    fn family_resolution() {
        assert_eq!(resolve_family("stub", "").unwrap(), Family::Stub);
        assert_eq!(resolve_family("sd_xl_base_1.0", "").unwrap(), Family::SdxlBase);
        assert_eq!(resolve_family("sdxl-base-1.0", "/models/sd_xl_base_1.0.safetensors").unwrap(), Family::SdxlBase);
        assert!(resolve_family("sd_xl_base_1.0", "/models/flux-2-klein-base-4b.safetensors").is_err());
        assert!(resolve_family("sdxl_unet_bf16", "").is_err());
        assert_eq!(resolve_family("flux-2-klein-base-9b", "").unwrap(), Family::Flux2KleinBase9b);
        assert_eq!(resolve_family("flux-2-klein-base-4b", "").unwrap(), Family::Flux2KleinBase4b);
        assert_eq!(resolve_family("FLUX.2-klein-base-4B", "").unwrap(), Family::Flux2KleinBase4b);
        assert!(resolve_family("flux-2-klein-base-4b", "/m/flux-2-klein-base-9b.safetensors").is_err());
        for model in ["flux-2-klein-4b", "flux-2-klein-9b", "flux-2-klein-9b-kv", "flux-2-klein-base-9b_fp8_e4m3fn", "flux2-dev"] {
            assert!(resolve_family(model, "").is_err(), "must not substitute Base weights for {model}");
        }
        assert_eq!(resolve_family("x", "/m/FLUX.2-klein-base-9B/flux-2-klein-base-9b.safetensors").unwrap(), Family::Flux2KleinBase9b);
        assert_eq!(resolve_family("krea2-turbo", "").unwrap(), Family::Krea2Turbo);
        assert_eq!(resolve_family("krea2-raw", "").unwrap(), Family::Krea2Raw);
        assert!(resolve_family("zimage", "").is_err());
    }

    #[test]
    fn png_text_chunk_roundtrip() {
        let png = png::encode_rgb8(2, 2, &[0u8; 12]);
        let out = png::with_text_chunk(&png, "serenity.genparams.v1", "{\"a\":1}").unwrap();
        assert_eq!(&out[..8], &png[..8]);
        assert!(out.windows(4).any(|w| w == b"tEXt"));
        assert_eq!(out.len(), png.len() + 12 + "serenity.genparams.v1".len() + 1 + 7);
    }

    #[test]
    fn klein_chains_select_exact_variant_and_refuse_substitution() {
        let directory = std::env::temp_dir().join(format!("difc-klein-chain-test-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let four = directory.join("flux-2-klein-base-4b.safetensors");
        let nine = directory.join("flux-2-klein-base-9b.safetensors");
        let vae = directory.join("vae.safetensors");
        for path in [&four, &nine, &vae] { fs::write(path, b"routing fixture only").unwrap(); }
        let profile = |checkpoint: &Path, cache: &str| json!({
            "model_dir": directory, "text_encoder_dir": directory, "tokenizer_dir": directory,
            "transformer_checkpoint": checkpoint, "vae_checkpoint": vae,
            "cache_dir": directory.join(cache), "extra_args": ["--resident-plan-mib", "20000"]
        });
        let mut cfg: Config = serde_json::from_value(json!({
            "compiler_build": directory,
            "flux2_klein_base_4b": profile(&four, "four-cache"),
            "flux2_klein_base_9b": profile(&nine, "nine-cache")
        })).unwrap();
        let mut p = JobParams::default();
        p.prompt = "a real prompt passed unchanged".into();
        p.width = 1024; p.height = 1024; p.steps = 50; p.cfg = 4.0;
        for (family, model, checkpoint) in [
            (Family::Flux2KleinBase4b, "klein4b", &four),
            (Family::Flux2KleinBase9b, "klein9b", &nine),
        ] {
            p.checkpoint_path = checkpoint.to_string_lossy().into_owned();
            let chain = build_flux2_chain(&cfg, &p, &directory, family).unwrap();
            let value = |flag: &str| chain.argv[chain.argv.iter().position(|v| v == flag).unwrap() + 1].clone();
            assert_eq!(chain.family, family);
            assert_eq!(value("--flux2-model"), model);
            assert_eq!(value("--transformer-checkpoint"), p.checkpoint_path);
            assert_eq!(value("--prompt"), p.prompt);
            assert_eq!(value("--steps"), "50");
            assert_eq!(value("--guidance"), "4");
            assert!(chain.argv.contains(&"--text-encoder-dir".into()));
        }
        p.checkpoint_path = nine.to_string_lossy().into_owned();
        assert!(build_flux2_chain(&cfg, &p, &directory, Family::Flux2KleinBase4b).err().unwrap().contains("substitute"));
        p.checkpoint_path.clear();
        p.cfg = f64::NAN;
        assert!(build_flux2_chain(&cfg, &p, &directory, Family::Flux2KleinBase4b).err().unwrap().contains("CFG"));
        p.cfg = 4.0;
        cfg.flux2_klein_base_4b.as_mut().unwrap().extra_args = vec!["--steps".into(), "4".into()];
        assert!(build_flux2_chain(&cfg, &p, &directory, Family::Flux2KleinBase4b).err().unwrap().contains("override --steps"));
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn sdxl_chain_preserves_prompt_negative_and_refuses_unsupported_requests() {
        let directory = std::env::temp_dir().join(format!("difc-sdxl-chain-test-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let checkpoint = directory.join("sd_xl_base_1.0.safetensors");
        let other = directory.join("other.safetensors");
        for path in [&checkpoint, &other, &directory.join("vocab.json"), &directory.join("merges.txt")] {
            fs::write(path, b"routing fixture only").unwrap();
        }
        let mut cfg: Config = serde_json::from_value(json!({"compiler_build": directory,
            "sdxl": {"checkpoint": checkpoint, "tokenizer_dir": directory, "cache_dir": directory.join("cache"),
                     "extra_args": ["--unet-dtype", "f16", "--vae-dtype", "bf16"]}})).unwrap();
        let mut p = JobParams::default();
        p.prompt = "a sunlit ceramic teapot".into(); p.negative = "blurry, text".into();
        p.width = 1024; p.height = 1024; p.steps = 50; p.cfg = 7.0;
        p.sampler = "euler".into(); p.scheduler = "normal".into(); p.seed = 4242;
        p.checkpoint_path = checkpoint.to_string_lossy().into_owned();
        let chain = build_sdxl_chain(&cfg, &p, &directory).unwrap();
        let value = |flag: &str| &chain.argv[chain.argv.iter().position(|v| v == flag).unwrap() + 1];
        assert!(chain.argv[0].ends_with("/difsdxlsample"));
        assert_eq!(value("--checkpoint"), &p.checkpoint_path);
        assert_eq!(value("--prompt"), &p.prompt); assert_eq!(value("--negative"), &p.negative);
        assert_eq!(value("--steps"), "50"); assert_eq!(value("--cfg"), "7");
        assert_eq!(value("--seed"), "4242"); assert_eq!(value("--width"), "1024");
        assert_eq!(chain.step_prefix, "SDXL_NATIVE_STEP");
        p.scheduler = "karras".into();
        assert!(build_sdxl_chain(&cfg, &p, &directory).err().unwrap().contains("normal"));
        p.scheduler = "normal".into(); p.cfg = f64::NAN;
        assert!(build_sdxl_chain(&cfg, &p, &directory).is_err());
        p.cfg = 7.0; p.init_image = "source.png".into();
        assert!(build_sdxl_chain(&cfg, &p, &directory).err().unwrap().contains("init_image"));
        p.init_image.clear(); p.checkpoint_path = other.to_string_lossy().into_owned();
        assert!(build_sdxl_chain(&cfg, &p, &directory).err().unwrap().contains("substitute"));
        p.checkpoint_path.clear();
        cfg.sdxl.as_mut().unwrap().extra_args = vec!["--steps".into(), "1".into()];
        assert!(build_sdxl_chain(&cfg, &p, &directory).err().unwrap().contains("extra_args"));
        cfg.sdxl.as_mut().unwrap().extra_args.clear();
        cfg.sdxl.as_mut().unwrap().tokenizer_dir = directory.join("missing").to_string_lossy().into_owned();
        assert!(build_sdxl_chain(&cfg, &p, &directory).err().unwrap().contains("artifact missing"));
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn refuses_unsupported_knobs_by_name() {
        let mut p = JobParams::default();
        p.clip_skip = 2;
        p.negative = "blurry".into();
        let u = unsupported_knobs(&p, false);
        assert!(u.iter().any(|s| s.starts_with("clip_skip=2")));
        assert!(u.iter().any(|s| s.starts_with("negative prompt")));
        assert!(unsupported_knobs(&JobParams::default(), false).is_empty());
    }
}
