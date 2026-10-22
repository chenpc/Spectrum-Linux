//! Thumbnail generation + in-memory cache.
//!
//! The service decodes through glycin once per file and keeps the **downscaled RGBA**, so a second
//! request for the same path is a cache hit (no decode). Target size is capped on the long side,
//! aspect ratio is preserved, and small images are never upscaled.
//!
//! The frame is not cached: glycin ignores a scale request for HEIC, so a phone photo decodes at
//! 8064×6048 (`R8G8B8`, ~146 MB). Caching that per file would exhaust memory long before the grid
//! is full, so `downscale()` squeezes it to the target size and the frame is dropped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Default cache size is unbounded; `with_capacity` adds LRU eviction.
use libglycin_rebind as gly;

/// A decoded, downscaled frame in tightly packed `R8G8B8A8` (row-major, no stride padding).
/// Centre-crop tightly packed RGBA to a square. The design's grid cells are square, so a 4:3 photo
/// loses its sides rather than being squashed, and the UI does not need UV cropping (egui 0.36 does
/// not expose `UvRect`).
pub fn crop_center_square(decoded: &Decoded) -> Decoded {
    let side = decoded.width.min(decoded.height) as usize;
    if side == 0 || decoded.rgba.len() < (decoded.width * decoded.height * 4) as usize {
        return decoded.clone();
    }
    if decoded.width == decoded.height {
        return decoded.clone();
    }
    let x_off = (decoded.width as usize - side) / 2;
    let y_off = (decoded.height as usize - side) / 2;
    let mut out = vec![0u8; side * side * 4];
    for row in 0..side {
        let from = ((y_off + row) * decoded.width as usize + x_off) * 4;
        let to = row * side * 4;
        out[to..to + side * 4].copy_from_slice(&decoded.rgba[from..from + side * 4]);
    }
    Decoded {
        width: side as u32,
        height: side as u32,
        rgba: out,
    }
}

#[derive(Clone)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Average `src` (row-major with `stride` bytes per row, `channels` of 3 or 4) into
/// `dst_w × dst_h` `R8G8B8A8`. Every destination pixel is the box average of its source block, so
/// a 49 MP photo costs one pass over the source rather than one per destination pixel lookup.
pub fn downscale(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    stride: u32,
    channels: usize,
    dst_w: u32,
    dst_h: u32,
) -> Vec<u8> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 || !(3..=4).contains(&channels) {
        return Vec::new();
    }
    let stride = stride as usize;
    let mut out = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    for oy in 0..dst_h as usize {
        let y0 = (oy * src_h as usize) / dst_h as usize;
        let y1 = ((oy + 1) * src_h as usize).max(y0 + 1) / dst_h as usize;
        for ox in 0..dst_w as usize {
            let x0 = (ox * src_w as usize) / dst_w as usize;
            let x1 = ((ox + 1) * src_w as usize).max(x0 + 1) / dst_w as usize;
            let mut sums = [0u64; 4];
            let mut count = 0u64;
            for y in y0..y1 {
                let row = y * stride;
                for x in x0..x1 {
                    let p = row + x * channels;
                    if p + channels > src.len() {
                        break;
                    }
                    for c in 0..channels {
                        sums[c] += src[p + c] as u64;
                    }
                    // A 3-channel source has no alpha: synthesise an opaque one, and only once.
                    if channels == 3 {
                        sums[3] += 255;
                    }
                    count += 1;
                }
            }
            let n = count.max(1);
            let at = (oy * dst_w as usize + ox) * 4;
            out[at] = (sums[0] / n) as u8;
            out[at + 1] = (sums[1] / n) as u8;
            out[at + 2] = (sums[2] / n) as u8;
            out[at + 3] = (sums[3] / n) as u8;
        }
    }
    out
}

/// Cap `width`/`height` to `max` on the long side, preserving aspect ratio (never upscale).
pub fn scale_within(width: u32, height: u32, max: u32) -> (u32, u32) {
    if width == 0 || height == 0 || max == 0 {
        return (width, height);
    }
    if width <= max && height <= max {
        return (width, height);
    }
    let scale = (max as f64 / width as f64).min(max as f64 / height as f64);
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

#[derive(Clone)]
pub struct Thumbnail {
    pub path: PathBuf,
    /// Target size the UI should reserve.
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    /// True when this call was served from the cache (no decode).
    pub cached: bool,
}

struct Cached {
    decoded: Decoded,
    thumb: Thumbnail,
}

pub struct ThumbnailService {
    max: u32,
    capacity: usize,
    cache: HashMap<PathBuf, Cached>,
    /// Access order, least-recent first.
    order: Vec<PathBuf>,
    hits: usize,
    decodes: usize,
    evictions: usize,
}

impl ThumbnailService {
    /// Unbounded cache.
    pub fn new(max: u32) -> Self {
        ThumbnailService {
            max,
            capacity: usize::MAX,
            cache: HashMap::new(),
            order: Vec::new(),
            hits: 0,
            decodes: 0,
            evictions: 0,
        }
    }

    /// Cache that evicts the least-recently-used entry once it exceeds `capacity`.
    pub fn with_capacity(max: u32, capacity: usize) -> Self {
        let mut s = ThumbnailService::new(max);
        s.capacity = capacity;
        s
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Change the LRU limit and trim immediately if the cache is already larger.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        while self.order.len() > self.capacity {
            let lru = self.order.remove(0);
            self.cache.remove(&lru);
            self.evictions += 1;
        }
    }

    /// Number of entries evicted by the LRU policy.
    pub fn evictions(&self) -> usize {
        self.evictions
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    /// Number of requests served from the cache.
    pub fn hits(&self) -> usize {
        self.hits
    }

    /// Number of glycin decodes performed.
    pub fn decodes(&self) -> usize {
        self.decodes
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Drop every cached entry (counters are kept).
    pub fn clear(&mut self) {
        self.cache.clear();
        self.order.clear();
    }

    pub fn is_cached(&self, path: &Path) -> bool {
        self.cache.contains_key(path)
    }

    /// Decode (or hit the cache) and return the downscaled pixels plus the size the UI should draw.
    pub fn get(&mut self, path: &Path) -> Result<(Decoded, Thumbnail), String> {
        if let Some(c) = self.cache.get(path) {
            let decoded = c.decoded.clone();
            let mut thumb = c.thumb.clone();
            thumb.cached = true;
            self.hits += 1;
            self.touch(path);
            return Ok((decoded, thumb));
        }

        let file = gio::File::for_path(path);
        let loader = gly::Loader::new(&file);
        loader.set_accepted_memory_formats(
            gly::MemoryFormatSelection::R8G8B8A8 | gly::MemoryFormatSelection::R8G8B8,
        );
        // EXIF orientation is applied by the loader, so the UI must not rotate again.
        loader.set_apply_transformations(true);

        let image = loader.load().map_err(|e| format!("{path:?}: {e}"))?;
        let frame = image.next_frame().map_err(|e| format!("{path:?}: {e}"))?;

        let source_width = frame.width();
        let source_height = frame.height();
        let (width, height) = scale_within(source_width, source_height, self.max);
        let channels = match frame.memory_format() {
            gly::MemoryFormat::R8g8b8a8 | gly::MemoryFormat::R8g8b8a8Premultiplied => 4,
            gly::MemoryFormat::R8g8b8 => 3,
            other => return Err(format!("{path:?}: unsupported memory format {other:?}")),
        };
        let rgba = downscale(
            &frame.buf_bytes(),
            source_width,
            source_height,
            frame.stride(),
            channels,
            width,
            height,
        );
        let decoded = Decoded {
            width,
            height,
            rgba,
        };
        let thumb = Thumbnail {
            path: path.to_path_buf(),
            width,
            height,
            source_width,
            source_height,
            cached: false,
        };
        self.decodes += 1;
        self.cache.insert(
            path.to_path_buf(),
            Cached {
                decoded: decoded.clone(),
                thumb: thumb.clone(),
            },
        );
        self.order.push(path.to_path_buf());
        while self.order.len() > self.capacity {
            let lru = self.order.remove(0);
            self.cache.remove(&lru);
            self.evictions += 1;
        }
        Ok((decoded, thumb))
    }

    /// Mark `path` as most-recently used.
    fn touch(&mut self, path: &Path) {
        if let Some(i) = self.order.iter().position(|p| p == path) {
            self.order.remove(i);
        }
        self.order.push(path.to_path_buf());
    }
}
