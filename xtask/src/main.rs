#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! Project tooling: `cargo xtask <command>`.
//!
//! - `design [--fig PATH] [--write]` — read the FigCraft document, write or verify
//!   `assets/design/observed.json`, and verify `assets/design/theme.json` against it.
//! - `theme [--write]` — write or verify `crates/spectrum-app/src/tokens.rs` from `theme.json`.
//! - `ci` — fmt, clippy `-D warnings`, tests, design and theme checks.
//! - `scan <DIR>` — run the real folder scan and report what it found, with timings.
//! - `control <PORT> <METHOD> [PARAMS_JSON]` — drive a running app (`--control <port>`) over its
//!   own channel: widget ids, commands, AccessKit-free inspection.
//! - `shot <PORT> [--out PATH] [--wait SECONDS]` — ask that app to screenshot itself and wait for
//!   the BMP it writes, so a UI change can actually be looked at.
//!
//! The design source is not committed to this repository. Its path comes from
//! `SPECTRUM_FIG`, defaulting to `../design/osx-photos-prototype.figcraft` next to the repo.

mod design;
mod ipc;
mod theme;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const OBSERVED: &str = "assets/design/observed.json";
const THEME: &str = "assets/design/theme.json";
const TOKENS: &str = "crates/spectrum-app/src/tokens.rs";
const DEFAULT_FIG: &str = "../design/osx-photos-prototype.figcraft";

const USAGE: &str = "usage: cargo xtask <design|theme|ci|scan|control|shot> [options]\n\n  design [--fig PATH] [--write]\n  theme [--write]\n  ci\n  scan <DIR>\n  control <PORT> <METHOD> [PARAMS_JSON]\n  shot <PORT> [--out PATH] [--wait SECONDS]\n";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str);
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    match command {
        Some("design") => report(run_design(&rest)),
        Some("theme") => report(run_theme(&rest)),
        Some("ci") => report(run_ci(&rest)),
        Some("scan") => run_scan(&rest),
        Some("control") => report_text(ipc::run_control(&rest)),
        Some("shot") => report_text(ipc::run_shot(&rest)),
        Some("help") | Some("-h") | Some("--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => report(Err(format!("unknown command `{other}\n\n{USAGE}"))),
        None => report(Err(format!("no command given\n\n{USAGE}"))),
    }
}

/// Report what `scan_folder` sees in a real folder: counts by kind, how many read as HDR, how
/// many have a sidecar, and how long it took. This is the cheapest way to tell whether a folder
/// scans correctly without opening the UI.
fn run_scan(args: &[&str]) -> ExitCode {
    let Some(dir) = args.first() else {
        return report(Err(format!("scan needs a directory\n\n{USAGE}")));
    };
    let path = Path::new(dir);
    if !path.is_dir() {
        return report(Err(format!("{} is not a directory", path.display())));
    }
    let started = std::time::Instant::now();
    let entries = spectrum_core::scan_folder(path);
    let elapsed = started.elapsed();

    let photos = entries
        .iter()
        .filter(|e| e.kind == spectrum_core::scan::MediaKind::Photo)
        .count();
    let videos = entries
        .iter()
        .filter(|e| e.kind == spectrum_core::scan::MediaKind::Video)
        .count();
    let hdr = entries
        .iter()
        .filter(|e| e.signal.as_ref().is_some_and(spectrum_core::is_hdr_signal))
        .count();
    let sidecars = entries.iter().filter(|e| e.edit.is_some()).count();
    let bytes: u64 = entries.iter().map(|e| e.size).sum();

    println!(
        "{}: {} entries ({photos} photos, {videos} videos), {hdr} HDR, {sidecars} sidecars, {} MiB in {:.1} ms",
        path.display(),
        entries.len(),
        bytes / (1024 * 1024),
        elapsed.as_secs_f64() * 1000.0
    );
    for entry in entries.iter().take(12) {
        let signal = entry
            .signal
            .as_ref()
            .map(|s| s.transfer.name())
            .unwrap_or("none");
        // The ICC profile is where Sony/Apple put HDR signalling when `nclx` says "unspecified",
        // so print what the profile actually carries rather than guessing from the transfer name.
        let icc = spectrum_core::detect_icc(&entry.path);
        let icc_text = match icc.as_ref() {
            Some(info) => {
                let name = info.name.as_deref().unwrap_or("(no desc tag)");
                let dmnd = info
                    .media_white_point
                    .map(|cd| format!(" {cd} cd/m2"))
                    .unwrap_or_default();
                let cicp = info
                    .cicp
                    .as_ref()
                    .map(|n| format!(" cicp p{} t{} m{}", n.primaries, n.transfer, n.matrix))
                    .unwrap_or_default();
                let gain = if info.gain_map { " gain-map(hdgm)" } else { "" };
                let desc = info
                    .name
                    .as_deref()
                    .filter(|n| !n.is_empty())
                    .map(|n| format!("\"{n}\""))
                    .unwrap_or_default();
                format!("{name}{dmnd}{cicp}{gain}{desc}")
            }
            None => "no iccp".to_string(),
        };
        println!(
            "  {:<28} {:?}  {signal:<12} icc: {icc_text}",
            entry.name(),
            entry.kind,
        );
    }
    if entries.len() > 12 {
        println!("  … {} more", entries.len() - 12);
    }
    ExitCode::SUCCESS
}

/// Print what the command produced, or the failure.
fn report_text(result: Result<String, String>) -> ExitCode {
    match result {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn report(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn fig_path(args: &[&str]) -> Result<PathBuf, String> {
    if let Some(path) = args.iter().find(|a| a.starts_with("--fig")) {
        let value = if let Some(split) = path.split_once('=') {
            split.1.to_string()
        } else {
            let index = args
                .iter()
                .position(|a| *a == "--fig")
                .unwrap_or(usize::MAX);
            args.get(index + 1)
                .map(|s| s.to_string())
                .ok_or("--fig needs a path")?
        };
        return Ok(PathBuf::from(value));
    }
    Ok(PathBuf::from(
        std::env::var("SPECTRUM_FIG").unwrap_or_else(|_| DEFAULT_FIG.to_string()),
    ))
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Write when asked, otherwise fail when the committed file no longer matches what is generated.
fn sync(path: &Path, generated: &str, write_it: bool, label: &str) -> Result<(), String> {
    if write_it {
        write(path, generated)?;
        println!("{label}: wrote {}", path.display());
        return Ok(());
    }
    match read(path) {
        Ok(existing) if existing == generated => {
            println!("{label}: {} matches its source", path.display());
            Ok(())
        }
        Ok(_) => Err(format!(
            "{} is stale: regenerate with `cargo xtask {} --write`",
            path.display(),
            label
        )),
        Err(_) => Err(format!(
            "{} is missing: generate it with `cargo xtask {} --write`",
            path.display(),
            label
        )),
    }
}

fn run_design(args: &[&str]) -> Result<(), String> {
    let write_it = args.contains(&"--write");
    for arg in args {
        if arg.starts_with("--")
            && !matches!(*arg, "--write" | "--fig")
            && !arg.starts_with("--fig=")
        {
            return Err(format!("design: unknown option `{arg}`\n\n{USAGE}"));
        }
    }
    let fig = fig_path(args)?;
    let doc = design::read_fig(&fig)?;
    let sha = design::fig_sha256(&fig)?;
    let observed = design::observe(&doc, &fig, &sha)?;
    let observed_text = serde_json::to_string_pretty(&observed)
        .map_err(|e| format!("serialize observed.json: {e}"))?
        + "\n";
    sync(&PathBuf::from(OBSERVED), &observed_text, write_it, "design")?;

    let theme_text = read(&PathBuf::from(THEME))?;
    let theme: serde_json::Value =
        serde_json::from_str(&theme_text).map_err(|e| format!("parse {THEME}: {e}"))?;
    design::check_theme(&theme, &observed)
        .map_err(|problems| format!("{THEME}: roles not in the design\n  {problems}"))?;
    println!("{THEME}: every colour role exists in the design's palette");
    Ok(())
}

fn run_theme(args: &[&str]) -> Result<(), String> {
    let write_it = args.contains(&"--write");
    if let Some(other) = args.iter().find(|a| **a != "--write") {
        return Err(format!("theme: unknown option `{other}`\n\n{USAGE}"));
    }
    let theme_text = read(&PathBuf::from(THEME))?;
    let theme: serde_json::Value =
        serde_json::from_str(&theme_text).map_err(|e| format!("parse {THEME}: {e}"))?;
    sync(
        &PathBuf::from(TOKENS),
        &theme::generate(&theme),
        write_it,
        "theme",
    )?;
    Ok(())
}

fn run_ci(args: &[&str]) -> Result<(), String> {
    let write_it = args.contains(&"--write");
    let design_args: &[&str] = if write_it { &["--write"] } else { &[] };
    let steps: [(&str, Vec<&str>); 3] = [
        ("fmt", vec!["fmt", "--all", "--", "--check"]),
        (
            "clippy",
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("test", vec!["test", "--workspace"]),
    ];
    for (name, cargo_args) in &steps {
        println!("\n=== ci: {name} ===");
        cargo(cargo_args)?;
    }
    println!("\n=== ci: design ===");
    run_design(design_args)?;
    println!("\n=== ci: theme ===");
    run_theme(design_args)?;
    println!("\nci: all steps passed");
    Ok(())
}

fn cargo(args: &[&str]) -> Result<(), String> {
    let status = Command::new("cargo")
        .args(args)
        .status()
        .map_err(|e| format!("run cargo {}: {e}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo {} failed ({status})", args.join(" ")))
    }
}
