use crate::crop::CropRect;

#[derive(Debug, Clone, PartialEq)]
pub enum EditOp {
    Crop(CropRect),
    /// 90 (CW) or -90 (CCW)
    Rotate(i32),
    FlipH,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompositeEdit {
    /// 0, 90, 180, 270
    pub rotation: i32,
    /// horizontal flip (applied before rotation)
    pub flip_h: bool,
    /// in rotated coordinate space; None = full image
    pub crop: Option<CropRect>,
}

impl CompositeEdit {
    pub fn from(ops: &[EditOp]) -> CompositeEdit {
        let mut rotation = 0;
        let mut flip_h = false;
        let mut crop: Option<CropRect> = None;

        for op in ops {
            match op {
                EditOp::Rotate(deg) => {
                    if let Some(c) = crop {
                        crop = Some(c.rotated(*deg));
                    }
                    rotation = (rotation + deg % 360 + 360) % 360;
                }
                EditOp::FlipH => {
                    if let Some(c) = crop {
                        crop = Some(c.flipped_h());
                    }
                    rotation = (360 - rotation) % 360;
                    flip_h = !flip_h;
                }
                // UI always draws a new crop on the full rotated image.
                EditOp::Crop(rect) => crop = Some(*rect),
            }
        }

        CompositeEdit {
            rotation,
            flip_h,
            crop,
        }
    }

    pub fn is_identity(&self) -> bool {
        self.rotation == 0 && !self.flip_h && self.crop.is_none()
    }

    /// Displayed size for a source image of `width`×`height`.
    /// Crop is stored in the rotated coordinate space, so rotation is applied first for sizing.
    pub fn display_size(&self, width: u32, height: u32) -> (u32, u32) {
        let (rw, rh) = if self.rotation == 90 || self.rotation == 270 {
            (height, width)
        } else {
            (width, height)
        };
        match self.crop {
            Some(c) => {
                let p = c.pixel_rect(rw as i32, rh as i32);
                (p.width.round() as u32, p.height.round() as u32)
            }
            None => (rw, rh),
        }
    }
}
