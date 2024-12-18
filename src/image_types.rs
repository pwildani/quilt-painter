use image::{ImageBuffer, Rgb};

#[derive(Clone)]
pub struct TextureImage(pub ImageBuffer<Rgb<u8>, Vec<u8>>);

#[derive(Clone)]
pub struct DepthImage(pub ImageBuffer<Rgb<u8>, Vec<u8>>);

#[derive(Clone)]
pub struct RgbdImage(pub ImageBuffer<Rgb<u8>, Vec<u8>>);

impl TextureImage {
    pub fn width(&self) -> u32 {
        self.0.width()
    }

    pub fn height(&self) -> u32 {
        self.0.height()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.0.dimensions()
    }
}

impl DepthImage {
    pub fn width(&self) -> u32 {
        self.0.width()
    }

    pub fn height(&self) -> u32 {
        self.0.height()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.0.dimensions()
    }
}

impl RgbdImage {
    pub fn split(self) -> (TextureImage, DepthImage) {
        let (width, height) = self.0.dimensions();
        let half_width = width / 2;

        let mut texture = ImageBuffer::new(half_width, height);
        let mut depth = ImageBuffer::new(half_width, height);

        for y in 0..height {
            for x in 0..half_width {
                texture.put_pixel(x, y, *self.0.get_pixel(x, y));
                depth.put_pixel(x, y, *self.0.get_pixel(x + half_width, y));
            }
        }

        (TextureImage(texture), DepthImage(depth))
    }

    pub fn width(&self) -> u32 {
        self.0.width()
    }

    pub fn height(&self) -> u32 {
        self.0.height()
    }
}

impl From<(TextureImage, DepthImage)> for RgbdImage {
    fn from((texture, depth): (TextureImage, DepthImage)) -> Self {
        let (width, height) = texture.0.dimensions();
        let mut combined = ImageBuffer::new(width * 2, height);

        for y in 0..height {
            for x in 0..width {
                combined.put_pixel(x, y, *texture.0.get_pixel(x, y));
                combined.put_pixel(x + width, y, *depth.0.get_pixel(x, y));
            }
        }

        RgbdImage(combined)
    }
}
