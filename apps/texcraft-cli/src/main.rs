//! TeXCraft without a window.
//!
//! - `texcraft-cli compile FOLDER [--output FILE.pdf]`: builds the project in FOLDER.
//! - `texcraft-cli mcp --automation-read-root DIR --automation-write-root DIR`: the agent tools
//!   over stdio; it refuses to start without both roots (AGENTS.md rule 18).

use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::json;
use texcraft_engine::Session;
use texcraft_engine::file_access::AutomationRoots;

const USAGE: &str = "usage:
  texcraft-cli compile FOLDER [--output FILE.pdf]
  texcraft-cli mcp --automation-read-root DIR --automation-write-root DIR
  texcraft-cli --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("compile") => compile(&args[1..]),
        Some("mcp") => mcp(&args[1..]),
        Some("--version") => {
            println!("texcraft-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// `--name VALUE` or `--name=VALUE`, repeatable.
fn values(args: &[String], name: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(v) = a.strip_prefix(&format!("{name}=")) {
            out.push(v.to_owned());
        } else if a == name {
            let v = args.get(i + 1).ok_or_else(|| format!("{name} needs a value"))?;
            out.push(v.clone());
            i += 1;
        }
        i += 1;
    }
    Ok(out)
}

fn mcp(args: &[String]) -> Result<(), String> {
    for a in args {
        let known = ["--automation-read-root", "--automation-write-root"].iter().any(|k| a == k || a.starts_with(&format!("{k}=")));
        if a.starts_with('-') && !known {
            return Err(format!("unknown option {a}\n{USAGE}"));
        }
    }
    let read: Vec<PathBuf> = values(args, "--automation-read-root")?.into_iter().map(PathBuf::from).collect();
    let write: Vec<PathBuf> = values(args, "--automation-write-root")?.into_iter().map(PathBuf::from).collect();
    let roots = AutomationRoots::new(&read, &write)?;
    texcraft_mcp::serve(roots).map_err(|e| format!("mcp: {e}"))
}

fn compile(args: &[String]) -> Result<(), String> {
    let folder = args.first().filter(|a| !a.starts_with('-')).ok_or_else(|| USAGE.to_owned())?;
    let output = values(args, "--output")?.pop();
    let mut session = Session::new();
    session.execute("project.open", &json!({"path": folder}))?;
    let result = session.execute("build.compile", &json!({"wait": true}))?;
    if let Some(diagnostics) = result.get("diagnostics").and_then(|d| d.as_array()) {
        for d in diagnostics {
            let place = match (d["file"].as_str(), d["line"].as_u64()) {
                (Some(f), Some(l)) => format!("{f}:{l}: "),
                (Some(f), None) => format!("{f}: "),
                _ => String::new(),
            };
            eprintln!("{place}{}: {}", d["severity"].as_str().unwrap_or("note"), d["message"].as_str().unwrap_or_default());
        }
    }
    let status = result["result"]["status"].as_str().unwrap_or("none");
    if status == "unavailable" {
        return Err(result["result"]["reason"].as_str().unwrap_or("no engine").to_owned());
    }
    let pdf = session.pdf().ok_or("the build wrote no PDF")?;
    let target = output.map_or_else(|| PathBuf::from(folder).join("output.pdf"), PathBuf::from);
    std::fs::write(&target, pdf.as_slice()).map_err(|e| format!("writing {}: {e}", target.display()))?;
    eprintln!("wrote {} ({} pages, {:.1} s)", target.display(), result["pages"], result["seconds"].as_f64().unwrap_or(0.0));
    Ok(())
}
