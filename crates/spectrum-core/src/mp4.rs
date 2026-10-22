//! Minimal ISO-BMFF video metadata: the sample-entry codec fourcc and the `mvhd` duration.
//!
//! Box walking only — we never decode pixels.

use crate::heif::container_skips;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VideoInfo {
    pub codec: Option<String>,
    pub duration_secs: Option<f64>,
}

impl VideoInfo {
    /// e.g. `hvc1 · 12.5s` or `1:05`.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(codec) = &self.codec {
            parts.push(codec.clone());
        }
        if let Some(d) = self.duration_secs {
            parts.push(format_duration(d));
        }
        parts.join(" · ")
    }
}

fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.1}s", secs)
    } else {
        format!(
            "{:02}:{:02}",
            (secs / 60.0).floor() as u64,
            (secs % 60.0).round() as u64
        )
    }
}

/// Parse boxes, but reject the ambiguous reading where a container's 4-byte version/flags is taken as
/// a box size of 0 ("extends to end of file"). Inside a container that produces a fake box whose
/// "type" is the first child's type, which is what made `mvhd` read the wrong fields.
fn strict_boxes(data: &[u8]) -> Option<Vec<([u8; 4], &[u8])>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 8 <= data.len() {
        let size = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        if size < 8 || i + size > data.len() {
            return None;
        }
        // the box type is at i+4..i+8; i..i+4 is the size field
        out.push((
            [data[i + 4], data[i + 5], data[i + 6], data[i + 7]],
            &data[i + 8..i + size],
        ));
        i += size;
    }
    if out.is_empty() || i != data.len() {
        return None;
    }
    Some(out)
}

fn walk<'a>(data: &'a [u8], out: &mut Vec<([u8; 4], &'a [u8])>) {
    let Some(parsed) = strict_boxes(data) else {
        return;
    };
    for (ty, content) in parsed {
        out.push((ty, content));
        if let Some(skips) = container_skips(&ty) {
            for skip in skips {
                if content.len() > skip {
                    walk(&content[skip..], out);
                }
            }
        }
    }
}

/// `mvhd` duration / timescale. Version 0 has 32-bit times; version 1 has 64-bit creation/modification
/// and duration, which shifts the fields.
fn mvhd_duration(content: &[u8]) -> Option<f64> {
    if content.len() < 20 {
        return None;
    }
    let (timescale, duration) = if content[0] == 1 {
        if content.len() < 28 {
            return None;
        }
        (
            u32::from_be_bytes([content[20], content[21], content[22], content[23]]),
            u64::from_be_bytes([
                content[24],
                content[25],
                content[26],
                content[27],
                content[28],
                content[29],
                content[30],
                content[31],
            ]),
        )
    } else {
        (
            u32::from_be_bytes([content[12], content[13], content[14], content[15]]),
            u32::from_be_bytes([content[16], content[17], content[18], content[19]]) as u64,
        )
    };
    if timescale == 0 || duration == 0 {
        return None;
    }
    Some(duration as f64 / timescale as f64)
}

pub fn parse_video_info(data: &[u8]) -> Option<VideoInfo> {
    let mut boxes_all = Vec::new();
    walk(data, &mut boxes_all);

    let mut codec = None;
    let mut duration = None;
    for (ty, content) in &boxes_all {
        if *ty == *b"mvhd" && duration.is_none() {
            duration = mvhd_duration(content);
        }
        if *ty == *b"stsd" && codec.is_none() {
            // stsd content = entry_count + entries, or version/flags + entry_count + entries when the
            // writer emitted a full box. Pick the interpretation whose entry_count matches the number
            // of boxes that follow, otherwise a garbage "box" can be read from the entry body.
            for count_at in [0, 4] {
                if content.len() < count_at + 8 {
                    continue;
                }
                let count = u32::from_be_bytes([
                    content[count_at],
                    content[count_at + 1],
                    content[count_at + 2],
                    content[count_at + 3],
                ]) as usize;
                let entries = match strict_boxes(&content[count_at + 4..]) {
                    Some(e) if e.len() == count => e,
                    _ => continue,
                };
                if count > 0 {
                    codec = Some(String::from_utf8_lossy(&entries[0].0).to_string());
                    break;
                }
            }
        }
    }

    if codec.is_none() && duration.is_none() {
        return None;
    }
    Some(VideoInfo {
        codec,
        duration_secs: duration,
    })
}
