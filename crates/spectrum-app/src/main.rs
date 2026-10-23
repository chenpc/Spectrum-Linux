#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;
use serde_json::json;
use spectrum_app::app::AppState;
use spectrum_app::control;

struct Runner(AppState);

impl eframe::App for Runner {
    /// Answer control requests before the pass, so a request and its reply straddle a real frame.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.0.logic(ctx);
        // While a control channel is open, also ask `winit` for a redraw. `Context::request_repaint`
        // only updates egui's schedule; on X11 nothing else pushes the event loop, so a window that
        // the compositor stops sending `RedrawRequested` for answers no request until the user
        // happens to move the mouse. `request_redraw` posts to the loop itself.
        if self.0.is_driven() {
            if let Some(window) = frame.winit_window() {
                window.request_redraw();
            }
        }
    }

    /// Deliver the queued control events as this pass's raw input.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.extend(self.0.take_events());
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

fn main() -> eframe::Result {
    let mut folder: Option<PathBuf> = None;
    let mut control_port: Option<u16> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--control" {
            // `--control 0` picks a free port and prints it.
            control_port = Some(match args.next().and_then(|v| v.parse::<u16>().ok()) {
                Some(port) => port,
                None => {
                    eprintln!("--control needs a port (use 0 for any free port)");
                    return Ok(());
                }
            });
        } else {
            folder = Some(PathBuf::from(arg));
        }
    }

    let mut state = AppState::new(folder.clone().unwrap_or_else(|| PathBuf::from(".")));
    state.set_store(spectrum_core::library_store::default_path());
    match state.restore_store() {
        Ok(saved) => {
            if !saved.folders.is_empty() {
                eprintln!(
                    "library: {} folder(s) from {}",
                    saved.folders.len(),
                    state
                        .store_path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                );
            }
        }
        Err(err) => eprintln!("library: {err} — keeping the command-line folder"),
    }
    // A folder on the command line wins: it is added if not already listed, then opened.
    if let Some(path) = folder.as_ref() {
        state.run("folder.add", json!({"path": path.to_string_lossy()}));
        state.run("folder.open", json!({"index": state.folder_index(path)}));
    }
    // Tiles 164 px on screen; keep the 64 most recent frames so scrolling back is a cache hit.
    state.set_thumb_worker(spectrum_app::thumbs::Worker::spawn(220, 64));
    let (handle, inbox) = control::channel(64);
    state.set_control(inbox);

    // Bind before the window: the chosen port is printed before anything else is drawn, so a
    // script can connect without guessing.
    let listener = control_port.map(|port| {
        TcpListener::bind(("127.0.0.1", port))
            .and_then(|sock| sock.local_addr().map(|addr| (sock, addr.port())))
    });
    let listener = match listener {
        None => None,
        Some(Ok(pair)) => Some(pair),
        Some(Err(err)) => {
            eprintln!(
                "control: cannot listen on port {}: {err}",
                control_port.unwrap_or(0)
            );
            None
        }
    };
    if let Some((_, port)) = listener.as_ref() {
        println!("control: listening on 127.0.0.1:{port}");
        std::io::stdout().flush().ok();
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_title("Spectrum"),
        ..Default::default()
    };

    eframe::run_native(
        "Spectrum",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            // Inter is the design's face (not vendored yet); a system CJK face covers glyphs it
            // lacks, so folder names in Chinese are not tofu boxes.
            let extra = std::env::var("SPECTRUM_CJK_FONT").ok();
            let candidates: Vec<&str> = extra
                .iter()
                .map(String::as_str)
                .chain(spectrum_app::theme::CJK_CANDIDATES.iter().copied())
                .collect();
            let cjk = spectrum_app::theme::install_fonts(
                &ctx,
                std::path::Path::new("assets/fonts/Inter.ttf"),
                &candidates,
            );
            println!(
                "fonts: cjk fallback = {}",
                cjk.unwrap_or_else(|| "none found".into())
            );
            std::io::stdout().flush().ok();
            if let Some((listener, _)) = listener {
                let serve_ctx = ctx.clone();
                std::thread::spawn(move || serve(listener, handle, serve_ctx));
                // `eframe` stops drawing when nothing wakes it, and a reply is only ever produced
                // inside a frame, so an idle window would never answer. Keep it drawing while the
                // control channel is open.
                let beat_ctx = ctx.clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(300));
                    beat_ctx.request_repaint();
                });
            }
            Ok(Box::new(Runner(state)))
        }),
    )
}

/// One JSON-lines connection: read a request, ask the app to answer it, write the reply.
/// The reply arrives from `AppState::logic`, so every request is answered in a real frame.
fn serve(listener: TcpListener, handle: control::Handle, ctx: egui::Context) {
    for incoming in listener.incoming() {
        let Ok(peer) = incoming else { continue };
        let handle = handle.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let Ok(reader) = peer.try_clone() else { return };
            let mut reader = BufReader::new(reader);
            let mut writer = peer;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let reply = match control::parse(&line) {
                    Some((id, method, params)) => handle.request_nudged(
                        id,
                        &method,
                        params,
                        &|| ctx.request_repaint(),
                        Duration::from_secs(5),
                    ),
                    None => json!({
                        "ok": false,
                        "error": "expected one JSON object per line: {\"id\":1,\"method\":...,\"params\":{...}}",
                    }),
                };
                if writer
                    .write_all(control::render(&reply).as_bytes())
                    .is_err()
                {
                    break;
                }
            }
        });
    }
}
