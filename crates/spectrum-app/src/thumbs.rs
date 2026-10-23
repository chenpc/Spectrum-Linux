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

/// A decode request: which file, how long its longest side may be, and how many frames of that size
/// may be kept. The grid asks for 164 px; the detail view asks for the frame at its own size, and
/// those must not share a cache or a full-size frame would push every tile out of it.
pub struct Request {
    pub path: PathBuf,
    pub max: u32,
    pub capacity: usize,
}

/// A finished decode: pixels on success, the loader's message on failure.
pub struct Job {
    pub path: PathBuf,
    pub max: u32,
    pub rgba: Option<(u32, u32, Vec<u8>)>,
    pub error: Option<String>,
}

/// The UI's end of the queue. Requests are fire-and-forget; results are drained once per frame.
pub struct Worker {
    default_max: u32,
    default_capacity: usize,
    requests: Sender<Request>,
    results: Receiver<Job>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Start one decoder thread. `max`/`capacity` are the defaults a request gets when it does not
    /// carry its own; requests for a different size get their own cache.
    pub fn spawn(max: u32, capacity: usize) -> Worker {
        let (requests_tx, requests_rx): (Sender<Request>, Receiver<Request>) = mpsc::channel();
        let (results_tx, results_rx): (Sender<Job>, Receiver<Job>) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut services: std::collections::HashMap<u32, ThumbnailService> =
                std::collections::HashMap::new();
            for request in requests_rx.iter() {
                let service = services.entry(request.max).or_insert_with(|| {
                    ThumbnailService::with_capacity(request.max, request.capacity)
                });
                let (rgba, error) = match service.get(&request.path) {
                    Ok((decoded, _)) => (Some((decoded.width, decoded.height, decoded.rgba)), None),
                    Err(message) => (None, Some(message)),
                };
                let job = Job {
                    path: request.path,
                    max: request.max,
                    rgba,
                    error,
                };
                if results_tx.send(job).is_err() {
                    break;
                }
            }
        });
        Worker {
            default_max: max,
            default_capacity: capacity,
            requests: requests_tx,
            results: results_rx,
            handle: Some(handle),
        }
    }

    /// Ask for a file at the worker's default size. The caller tracks what it already asked for, so
    /// this never queues twice.
    pub fn request(&self, path: PathBuf) {
        self.requests
            .send(Request {
                path,
                max: self.default_max,
                capacity: self.default_capacity,
            })
            .ok();
    }

    /// Ask for a file at a specific size and cache budget.
    pub fn request_at(&self, path: PathBuf, max: u32, capacity: usize) {
        self.requests
            .send(Request {
                path,
                max,
                capacity,
            })
            .ok();
    }

    /// A second sender, so the UI thread can hold it without borrowing the worker.
    pub fn sender(&self) -> Sender<Request> {
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
