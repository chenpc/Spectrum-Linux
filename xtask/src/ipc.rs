//! `cargo xtask control` / `cargo xtask shot` — talk to the running app over its control channel.
//!
//! `cargo xtask control 43137 ui.inspect`
//! `cargo xtask control 43137 ui.click '{"id":"segment:view:Months"}'`
//! `cargo xtask shot 43137 --out /tmp/library.bmp`
//!
//! Everything goes through the app's own channel (`--control <port>`): widget ids, commands, and
//! egui's own `ViewportCommand::Screenshot`. Nothing here reaches into the process, and nothing
//! synthesises OS input, so what a script sees is what a click does.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Send one request line and read one reply line.
pub fn request(port: u16, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let stream = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("cannot connect to 127.0.0.1:{port}: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("cannot set a read timeout: {e}"))?;
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);

    let line = json!({"id": 1, "method": method, "params": params}).to_string();
    writer
        .write_all(line.as_bytes())
        .and_then(|_| writer.write_all(b"\n"))
        .map_err(|e| format!("cannot send the request: {e}"))?;

    let mut reply = String::new();
    match reader.read_line(&mut reply) {
        Ok(0) => Err("the app closed the connection without answering".to_string()),
        Ok(_) => serde_json::from_str(&reply).map_err(|e| format!("bad reply `{reply}`: {e}")),
        Err(e) => Err(format!("no reply within {}: {e}", timeout.as_secs())),
    }
}

/// Ask the app to screenshot itself, then wait for the file it writes.
pub fn shot(port: u16, out: &Path, wait: Duration) -> Result<String, String> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let path = out.display().to_string();
    // A stale file would make "it exists" a lie, so drop it before asking.
    if out.exists() {
        std::fs::remove_file(out).map_err(|e| format!("cannot remove {path}: {e}"))?;
    }

    let reply = request(port, "ui.screenshot", json!({"path": path}), wait)?;
    if reply["ok"] != json!(true) {
        return Err(format!(
            "ui.screenshot refused: {}",
            reply["error"].as_str().unwrap_or("(no error given)")
        ));
    }

    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if let Ok(meta) = std::fs::metadata(out) {
            if meta.len() > 0 {
                return Ok(format!("{path}: {} bytes", meta.len()));
            }
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    Err(format!(
        "no image written to {path} within {}s",
        wait.as_secs()
    ))
}

/// `cargo xtask shot <port> [--out PATH] [--wait SECONDS]`
pub fn run_shot(args: &[&str]) -> Result<String, String> {
    let port = port(args)?;
    let mut out = PathBuf::from("target/shot.bmp");
    let mut wait = Duration::from_secs(5);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if *arg == "--out" {
            out = PathBuf::from(rest.next().ok_or("--out needs a path")?);
        } else if *arg == "--wait" {
            let secs: u64 = rest
                .next()
                .ok_or("--wait needs seconds")?
                .parse()
                .map_err(|_| "--wait wants a number of seconds")?;
            wait = Duration::from_secs(secs);
        }
    }
    shot(port, &out, wait)
}

/// `cargo xtask control <port> <method> [params-json]`
pub fn run_control(args: &[&str]) -> Result<String, String> {
    let port = port(args)?;
    let mut rest = args.iter().skip(1);
    let method = rest
        .next()
        .ok_or("control needs a method (ui.inspect, ui.click, …)")?;
    let params: Value = match rest.next() {
        Some(text) => {
            serde_json::from_str(text).map_err(|e| format!("params are not JSON: {e}"))?
        }
        None => json!({}),
    };
    let reply = request(port, method, params, Duration::from_secs(5))?;
    Ok(serde_json::to_string_pretty(&reply).unwrap_or_else(|_| reply.to_string()))
}

fn port(args: &[&str]) -> Result<u16, String> {
    let first = args.first().ok_or(
        "the first argument is the control port (`control: listening on …` in the app's log)",
    )?;
    first
        .parse()
        .map_err(|_| format!("`{first}` is not a port number"))
}
