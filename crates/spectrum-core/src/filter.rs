//! Display filter for the list/grid. Like `sort`, it returns indices into the scanned entries so
//! the UI can filter without re-scanning or cloning the whole list.

use crate::hdr::{is_hdr_signal, TransferFunction};
use crate::scan::{MediaEntry, MediaKind};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// None = both kinds.
    pub kind: Option<MediaKind>,
    /// Only files whose signalling is HDR (gain map / PQ / HLG).
    pub hdr_only: bool,
    /// None = any transfer function.
    pub transfer: Option<TransferFunction>,
    /// `YYYY-MM` month key (or `unknown`); None = any month.
    pub month: Option<String>,
}

impl Filter {
    pub fn matches(&self, e: &MediaEntry) -> bool {
        if let Some(k) = self.kind {
            if e.kind != k {
                return false;
            }
        }
        if self.hdr_only {
            match &e.signal {
                Some(s) => {
                    if !is_hdr_signal(s) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        if let Some(t) = self.transfer {
            match &e.signal {
                Some(s) => {
                    if s.transfer != t {
                        return false;
                    }
                }
                None => return false,
            }
        }
        if let Some(m) = &self.month {
            match e.mtime {
                Some(t) => {
                    if crate::timeline::month_key(t) != *m {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    pub fn is_empty(&self) -> bool {
        self.kind.is_none() && !self.hdr_only && self.transfer.is_none() && self.month.is_none()
    }

    pub fn label(&self) -> String {
        let mut parts = vec![self
            .kind
            .map(|k| k.label())
            .unwrap_or("all kinds")
            .to_string()];
        if self.hdr_only {
            parts.push("HDR only".to_string());
        }
        if let Some(t) = self.transfer {
            parts.push(t.name().to_string());
        }
        if let Some(m) = &self.month {
            parts.push(m.clone());
        }
        parts.join(" · ")
    }
}

/// Indices into `entries` that pass `filter`, in the original (scan) order.
pub fn filter_entries(entries: &[MediaEntry], filter: &Filter) -> Vec<usize> {
    if filter.is_empty() {
        return (0..entries.len()).collect();
    }
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| filter.matches(e))
        .map(|(i, _)| i)
        .collect()
}
