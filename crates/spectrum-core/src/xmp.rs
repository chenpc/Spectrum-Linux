use std::path::{Path, PathBuf};

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::crop::CropRect;
use crate::edit::CompositeEdit;
use crate::orientation::{exif_orientation_to_transform, transform_to_exif_orientation};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SidecarData {
    pub rotation: i32,
    pub flip_h: bool,
    pub crop: Option<CropRect>,
    pub gyro_config: Option<String>,
}

impl SidecarData {
    /// The edit as the display pipeline expects it (crop already in rotated space).
    pub fn edit(&self) -> CompositeEdit {
        CompositeEdit {
            rotation: self.rotation,
            flip_h: self.flip_h,
            crop: self.crop,
        }
    }
}

/// Sidecar path: `{name}.{ext}.xmp` — preserves original extension to distinguish photo vs video.
pub fn sidecar_path_for(image_path: &Path) -> PathBuf {
    let mut p = image_path.as_os_str().to_os_string();
    p.push(".xmp");
    PathBuf::from(p)
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn format_double(value: f64) -> String {
    let s = format!("{:.6}", value);
    if s.contains('.') {
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        return trimmed.to_string();
    }
    s
}

fn escape_xml_attribute(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn write_sidecar(
    image_path: &Path,
    edit: &CompositeEdit,
    original_orientation: i32,
    gyro_config: Option<&str>,
) -> std::io::Result<PathBuf> {
    let (exif_r, exif_h) = exif_orientation_to_transform(original_orientation);
    let edit_r = edit.rotation;
    let edit_h = edit.flip_h;

    let (combined_r, combined_h) = if !edit_h {
        ((exif_r + edit_r) % 360, exif_h)
    } else {
        ((edit_r - exif_r + 360) % 360, !exif_h)
    };
    let ori = transform_to_exif_orientation(combined_r, combined_h);

    let has_crop = edit.crop.is_some();
    let (crop_top, crop_left, crop_bottom, crop_right) = match edit.crop {
        Some(c) => (c.y, c.x, c.y + c.height, c.x + c.width),
        None => (0.0, 0.0, 1.0, 1.0),
    };

    let mut xml = String::new();
    xml.push_str("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    xml.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Spectrum 1.0\">\n");
    xml.push_str(" <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    xml.push_str("  <rdf:Description rdf:about=\"\"\n");
    xml.push_str("    xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\"\n");
    xml.push_str("    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n");
    xml.push_str("    xmlns:spectrum=\"http://spectrum.app/ns/1.0/\"\n");
    xml.push_str(&format!("    tiff:Orientation=\"{}\"\n", ori));
    xml.push_str(&format!(
        "    crs:HasCrop=\"{}\"\n",
        if has_crop { "True" } else { "False" }
    ));
    xml.push_str(&format!(
        "    crs:CropTop=\"{}\"\n",
        format_double(crop_top)
    ));
    xml.push_str(&format!(
        "    crs:CropLeft=\"{}\"\n",
        format_double(crop_left)
    ));
    xml.push_str(&format!(
        "    crs:CropBottom=\"{}\"\n",
        format_double(crop_bottom)
    ));
    xml.push_str(&format!(
        "    crs:CropRight=\"{}\"\n",
        format_double(crop_right)
    ));
    xml.push_str("    crs:CropAngle=\"0\"\n");
    if let Some(g) = gyro_config {
        xml.push_str(&format!(
            "    spectrum:GyroConfig=\"{}\"\n",
            escape_xml_attribute(g)
        ));
    }
    xml.push_str("  />\n");
    xml.push_str(" </rdf:RDF>\n");
    xml.push_str("</x:xmpmeta>\n");
    xml.push_str("<?xpacket end=\"w\"?>\n");

    let path = sidecar_path_for(image_path);
    std::fs::write(&path, xml)?;
    Ok(path)
}

pub fn read_sidecar(image_path: &Path, original_orientation: i32) -> Option<SidecarData> {
    let path = sidecar_path_for(image_path);
    if !path.exists() {
        return None;
    }
    let data = std::fs::read(&path).ok()?;
    let attrs = find_description_attrs(&data)?;

    let get = |name: &str| -> Option<String> {
        if let Some(v) = attrs.get(name) {
            return Some(v.clone());
        }
        let local = name.split(':').next_back().unwrap_or(name);
        for (k, v) in &attrs {
            if k.ends_with(local) {
                return Some(v.clone());
            }
        }
        None
    };

    let xmp_ori: i32 = get("tiff:Orientation")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let (xmp_r, xmp_h) = exif_orientation_to_transform(xmp_ori);
    let (exif_r, exif_h) = exif_orientation_to_transform(original_orientation);

    let (edit_r, edit_h) = if xmp_h == exif_h {
        (((xmp_r - exif_r + 360) % 360), false)
    } else {
        (((xmp_r + exif_r) % 360), true)
    };

    let has_crop = get("crs:HasCrop")
        .map(|s| s.to_lowercase() == "true")
        .unwrap_or(false);
    let mut crop = None;
    if has_crop {
        let top = round6(get("crs:CropTop")?.parse::<f64>().ok()?);
        let left = round6(get("crs:CropLeft")?.parse::<f64>().ok()?);
        let bottom = round6(get("crs:CropBottom")?.parse::<f64>().ok()?);
        let right = round6(get("crs:CropRight")?.parse::<f64>().ok()?);
        let w = round6(right - left);
        let h = round6(bottom - top);
        if w > 0.0 && h > 0.0 {
            crop = Some(CropRect::new(left, top, w, h));
        }
    }

    let gyro_config = get("spectrum:GyroConfig");

    if edit_r == 0 && !edit_h && crop.is_none() && gyro_config.is_none() {
        return None;
    }

    Some(SidecarData {
        rotation: edit_r,
        flip_h: edit_h,
        crop,
        gyro_config,
    })
}

pub fn delete_sidecar(image_path: &Path) {
    let path = sidecar_path_for(image_path);
    let _ = std::fs::remove_file(path);
}

fn find_description_attrs(xml: &[u8]) -> Option<HashMap<String, String>> {
    let mut reader = Reader::from_reader(xml);
    loop {
        match reader.read_event().ok()? {
            // `<rdf:Description ... />` is self-closing, so quick-xml emits Event::Empty,
            // not Event::Start — matching only Start made every read return None.
            Event::Start(e) | Event::Empty(e)
                if e.name().local_name().as_ref() == b"Description" =>
            {
                let mut attrs = HashMap::new();
                for attr in e.attributes() {
                    let attr = attr.ok()?;
                    let name = String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
                    let value = attr.unescape_value().ok()?.to_string();
                    attrs.insert(name, value);
                }
                return Some(attrs);
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}
