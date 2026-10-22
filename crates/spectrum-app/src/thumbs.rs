#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! Background thumbnail decoding for the grid.
//!
//! Decoding is not a frame operation: a phone HEIC costs ~850 ms and a 146 MB frame here
//! (`libglycin` ignores a scale request for HEIC, so the decode runs at full size and
//! `spectrum_core::thumbnail::downscale` squeezes it afterwards). One worker thread keeps a single
//! such frame alive at a time, and the UI keeps drawing while it waits: tiles stay in the design's
//! placeholder colour until a texture arrives.
//!
//! The worker is created only by the real window (`main.rs`). Tests never spawn it, so snapshots
//! cannot depend on decode timing.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use spectrum_core::thumbnail::ThumbnailService;

/// A finished decode: pixels on success, the loader's message on failure.
pub struct Job {
    pub path: PathBuf,
    pub rgba: Option<(u32, u32, Vec<u8>)>,
    pub error: Option<String>,
}

/// The UI's end of the queue. Requests are fire-and-forget; results are drained once per frame.
pub struct Worker {
    requests: Sender<PathBuf>,
    results: Receiver<Job>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Start one decoder thread with an LRU cache of `capacity` downscaled frames.
    pub fn spawn(max: u32, capacity: usize) -> Worker {
        let (requests_tx, requests_rx): (Sender<PathBuf>, Receiver<PathBuf>) = mpsc::channel();
        let (results_tx, results_rx): (Sender<Job>, Receiver<Job>) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut service = ThumbnailService::with_capacity(max, capacity);
            for path in requests_rx.iter() {
                let job = match service.get(&path) {
                    Ok((decoded, _)) => Job {
                        path,
                        rgba: Some((decoded.width, decoded.height, decoded.rgba)),
                        error: None,
                    },
                    Err(message) => Job {
                        path,
                        rgba: None,
                        error: Some(message),
                    },
                };
                if results_tx.send(job).is_err() {
                    break;
                }
            }
        });
        Worker {
            requests: requests_tx,
            results: results_rx,
            handle: Some(handle),
        }
    }

    /// Ask for a file. The caller tracks what it already asked for, so this never queues twice.
    pub fn request(&self, path: PathBuf) {
        self.requests.send(path).ok();
    }

    /// A second sender, so the UI thread can hold it without borrowing the worker.
    pub fn sender(&self) -> Sender<PathBuf> {
        self.requests.clone()
    }

    /// Take everything finished so far, without blocking the frame.
    pub fn take(&self) -> Vec<Job> {
        let mut out = Vec::new();
        while let Ok(job) = self.results.try_recv() {
            out.push(job);
        }
        out
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing the request channel lets the thread finish its current decode and exit; dropping
        // the join handle detaches it, which is what a window closing wants.
        self.handle.take();
    }
}
