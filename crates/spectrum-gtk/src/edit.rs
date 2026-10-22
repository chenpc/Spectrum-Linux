//! Apply a `CompositeEdit` (crop / rotate / flip) to a decoded glycin frame.
//!
//! Convention matches `orientation.rs`: flip first, then rotate. gdk-pixbuf is used only as the
//! pixel buffer here — `gdk::Texture` has no transform API, and `Pixbuf` can flip/rotate in place.

use gdk4 as gdk;
use gdk_pixbuf as pixbuf;
use libglycin_rebind as gly;
use libglycin_gtk4_rebind as gly_gtk;
use spectrum_core::edit::CompositeEdit;

fn bytes_per_pixel(fmt: gly::MemoryFormat) -> Option<bool> {
    match fmt {
        gly::MemoryFormat::R8g8b8 => Some(false),
        gly::MemoryFormat::R8g8b8a8 => Some(true),
        _ => None,
    }
}

pub fn pixbuf_from_frame(frame: &gly::Frame) -> Result<pixbuf::Pixbuf, String> {
    let has_alpha = bytes_per_pixel(frame.memory_format()).ok_or_else(|| {
        format!("unsupported memory format {:?}", frame.memory_format())
    })?;
    Ok(pixbuf::Pixbuf::from_bytes(
        &frame.buf_bytes(),
        pixbuf::Colorspace::Rgb,
        has_alpha,
        8,
        frame.width() as i32,
        frame.height() as i32,
        frame.stride() as i32,
    ))
}

pub fn apply_edit(frame: &gly::Frame, edit: &CompositeEdit) -> Result<gdk::Texture, String> {
    if edit.is_identity() {
        return Ok(gly_gtk::frame_get_texture(frame));
    }
    let mut pb = pixbuf_from_frame(frame)?;
    if edit.flip_h {
        pb = pb.flip(true).ok_or("flip failed".to_string())?;
    }
    let angle = match edit.rotation {
        0 => pixbuf::PixbufRotation::None,
        90 => pixbuf::PixbufRotation::Clockwise,
        180 => pixbuf::PixbufRotation::Upsidedown,
        270 => pixbuf::PixbufRotation::Counterclockwise,
        other => return Err(format!("unsupported rotation {other}")),
    };
    pb = pb.rotate_simple(angle).ok_or("rotate failed".to_string())?;
    if let Some(c) = edit.crop {
        // crop is stored in the rotated coordinate space; after rotate_simple the buffer is already
        // in that space, so the origin must use the rotated dimensions too (not the source ones).
        let (rw, rh) = if edit.rotation == 90 || edit.rotation == 270 {
            (frame.height(), frame.width())
        } else {
            (frame.width(), frame.height())
        };
        let p = c.pixel_rect(rw as i32, rh as i32);
        pb = pb.new_subpixbuf(
            p.x.round() as i32,
            p.y.round() as i32,
            p.width.round() as i32,
            p.height.round() as i32,
        );
    }
    Ok(gdk::Texture::for_pixbuf(&pb))
}
