use std::time::{Duration, SystemTime};

/// Completion messages auto-clear after this long (macOS `StatusBarModel` uses a 10 s Task.sleep).
pub const DEFAULT_TTL: Duration = Duration::from_secs(10);

/// Background/foreground task status shown in the status bar (macOS DESIGN.md §4.11).
///
/// The 10 s timers are modelled as explicit deadlines rather than real sleeps: `finish` /
/// `finish_global` take a `now` clock and `tick(now)` clears whatever expired. That keeps the model
/// deterministic for tests — no test has to wait 10 seconds, and nothing depends on wall-clock time.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusBarModel {
    pub active: bool,
    pub label: String,
    /// None = indeterminate task (spinner, no count).
    pub total: Option<usize>,
    pub done: usize,
    pub done_message: Option<String>,
    pub global_label: Option<String>,
    message_expiry: Option<SystemTime>,
    global_expiry: Option<SystemTime>,
    ttl: Duration,
}

impl Default for StatusBarModel {
    fn default() -> StatusBarModel {
        StatusBarModel::new()
    }
}

impl StatusBarModel {
    pub fn new() -> StatusBarModel {
        StatusBarModel {
            active: false,
            label: String::new(),
            total: None,
            done: 0,
            done_message: None,
            global_label: None,
            message_expiry: None,
            global_expiry: None,
            ttl: DEFAULT_TTL,
        }
    }

    pub fn with_ttl(ttl: Duration) -> StatusBarModel {
        let mut m = StatusBarModel::new();
        m.ttl = ttl;
        m
    }

    /// Start an indeterminate task. Starting a task cancels the pending completion timer, so the
    /// previous completion message is replaced rather than lingering for the rest of its TTL.
    pub fn begin(&mut self, label: &str) {
        self.active = true;
        self.label = label.to_string();
        self.total = None;
        self.done = 0;
        self.done_message = None;
        self.message_expiry = None;
    }

    /// Start a determinate task (progress bar with a known total).
    pub fn begin_total(&mut self, label: &str, total: usize) {
        self.begin(label);
        self.total = Some(total);
    }

    /// Report progress. `done` is clamped to `total` for a determinate task so a stray update
    /// cannot show "12/10".
    pub fn update(&mut self, done: usize, label: &str) {
        self.done = match self.total {
            Some(total) => done.min(total),
            None => done,
        };
        self.label = label.to_string();
    }

    /// Complete the task and show `message` until `now + ttl`.
    pub fn finish(&mut self, message: &str, now: SystemTime) {
        self.active = false;
        self.done_message = Some(message.to_string());
        self.message_expiry = Some(now + self.ttl);
    }

    /// Show a background task label (no progress). It stays until `finish_global` or `clear`.
    pub fn set_global(&mut self, label: &str) {
        self.global_label = Some(label.to_string());
        self.global_expiry = None;
    }

    /// Complete the background task: the label becomes `message` and expires at `now + ttl`.
    pub fn finish_global(&mut self, message: &str, now: SystemTime) {
        self.global_label = Some(message.to_string());
        self.global_expiry = Some(now + self.ttl);
    }

    /// Clear expired messages; returns true when something changed (the UI should refresh).
    pub fn tick(&mut self, now: SystemTime) -> bool {
        let mut changed = false;
        if let Some(expiry) = self.message_expiry {
            if now >= expiry {
                self.message_expiry = None;
                self.done_message = None;
                changed = true;
            }
        }
        if let Some(expiry) = self.global_expiry {
            if now >= expiry {
                self.global_expiry = None;
                self.global_label = None;
                changed = true;
            }
        }
        changed
    }

    /// macOS `isVisible`: active task, a completion message, or a background label.
    pub fn is_visible(&self) -> bool {
        self.active || self.done_message.is_some() || self.global_label.is_some()
    }

    /// The text the status bar renders. An active task wins over a completion message, which wins
    /// over the background label.
    pub fn text(&self) -> String {
        if self.active {
            return match self.total {
                Some(total) => format!("{} {}/{}", self.label, self.done, total),
                None => self.label.clone(),
            };
        }
        if let Some(message) = &self.done_message {
            return message.clone();
        }
        self.global_label.clone().unwrap_or_default()
    }

    pub fn clear(&mut self) {
        self.active = false;
        self.label = String::new();
        self.total = None;
        self.done = 0;
        self.done_message = None;
        self.global_label = None;
        self.message_expiry = None;
        self.global_expiry = None;
    }
}
