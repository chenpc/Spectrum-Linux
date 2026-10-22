/// Normalised crop rectangle (0..1), matching `CropRect` in the macOS reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl CropRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        CropRect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn pixel_rect(&self, image_width: i32, image_height: i32) -> PixelRect {
        PixelRect {
            x: self.x * image_width as f64,
            y: self.y * image_height as f64,
            width: self.width * image_width as f64,
            height: self.height * image_height as f64,
        }
    }

    pub fn flipped_h(&self) -> CropRect {
        CropRect::new(1.0 - self.x - self.width, self.y, self.width, self.height)
    }

    /// Transform crop coordinates when the image is rotated by `degrees` (CW).
    pub fn rotated(&self, degrees: i32) -> CropRect {
        let norm = degrees.rem_euclid(360);
        match norm {
            // 90° CW: (x, y, w, h) -> (1-y-h, x, h, w)
            90 => CropRect::new(1.0 - self.y - self.height, self.x, self.height, self.width),
            180 => CropRect::new(
                1.0 - self.x - self.width,
                1.0 - self.y - self.height,
                self.width,
                self.height,
            ),
            // 270° CW (= 90° CCW): (x, y, w, h) -> (y, 1-x-w, h, w)
            270 => CropRect::new(self.y, 1.0 - self.x - self.width, self.height, self.width),
            _ => *self,
        }
    }
}
