use difc_config::{get, h3_task_missing, load, repository_root};
fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or("usage: difc-config resolve|get KEY|check-h3 [quant] [--config PATH]")?;
    let rest: Vec<String> = args.collect();
    let path = rest.windows(2).find(|w| w[0] == "--config").map(|w| w[1].clone().into()).unwrap_or_else(difc_config::config_path);
    let doc = load(&path, &repository_root())?;
    match command.as_str() {
        "register" => difc_config::register(&doc)?,
        "resolve" => println!("{}", serde_json::to_string_pretty(&doc).unwrap()),
        "get" => { let v = get(&doc, rest.first().ok_or("get requires a key")?)?;
            if let Some(s) = v.as_str() { println!("{s}"); } else { println!("{v}"); }
        },
        "check-h3" => {
            let quant = rest.first().filter(|s| !s.starts_with("--")).map(String::as_str).unwrap_or("int8");
            let task = rest.windows(2).find(|w| w[0] == "--task").map(|w| w[1].as_str()).unwrap_or("t2va");
            let missing = h3_task_missing(&doc, task, quant);
            println!("{}", serde_json::json!({"ready":missing.is_empty(), "missing":missing}));
            if !missing.is_empty() { return Err("H3 prerequisites missing; no generation started".into()); }
        },
        _ => return Err(format!("unknown command: {command}")),
    }
    Ok(())
}
fn main() { if let Err(e) = run() { eprintln!("difc-config: {e}"); std::process::exit(1); } }
