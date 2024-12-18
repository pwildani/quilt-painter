use image::{ImageBuffer, Rgb};

#[cfg(not(feature = "captions"))]
#[derive(Default, Clone, Debug)]
pub struct CaptionConfig();

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Position {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
}

impl Default for Position {
    fn default() -> Self {
        Position::BottomCenter
    }
}

#[cfg(feature = "captions")]
#[derive(Default, Clone, Debug)]
pub struct CaptionConfig {
    pub text: Option<String>,
    pub size: u32,
    pub position: Position,
}

#[cfg(feature = "captions")]
impl CaptionConfig {
    pub fn new(text: Option<String>, size: u32, position: Position) -> Self {
        Self {
            text,
            size,
            position,
        }
    }
}
#[cfg(not(feature = "captions"))]
pub fn draw_caption(
    view: ImageBuffer<Rgb<u8>, Vec<u8>>,
    _caption: CaptionConfig,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    view
}

#[cfg(feature = "captions")]
pub fn draw_caption(
    mut view: ImageBuffer<Rgb<u8>, Vec<u8>>,
    caption: CaptionConfig,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    if let Some(text) = caption.text {
        use rusttype::{Font, Scale};

        // Load font
        let font_data = include_bytes!("../assets/font.ttf");
        let font = Font::try_from_bytes(font_data as &[u8]).unwrap();

        // Prepare scale and color
        let scale = Scale::uniform(caption.size as f32);
        let color = Rgb([255, 255, 255]); // White text

        // Calculate text size
        let v_metrics = font.v_metrics(scale);
        let glyphs: Vec<_> = font
            .layout(&text, scale, rusttype::Point { x: 0.0, y: 0.0 })
            .collect();
        let text_width = glyphs
            .iter()
            .next_back()
            .map(|g| g.position().x + g.unpositioned().h_metrics().advance_width)
            .unwrap_or(0.0) as i32;
        let text_height = (v_metrics.ascent - v_metrics.descent).ceil() as i32;

        let (x, y) = match caption.position {
            Position::TopLeft => (10, 10),
            Position::TopCenter => ((view.width() as i32 - text_width) / 2, 10),
            Position::TopRight => (view.width() as i32 - text_width - 10, 10),
            Position::BottomLeft => (10, view.height() as i32 - text_height - 10),
            Position::BottomCenter => (
                (view.width() as i32 - text_width) / 2,
                view.height() as i32 - text_height - 10,
            ),
        };

        // Draw text
        for glyph in glyphs {
            if let Some(bounding_box) = glyph.pixel_bounding_box() {
                glyph.draw(|gx, gy, intensity| {
                    let gx = gx as i32 + bounding_box.min.x + x;
                    let gy = gy as i32 + bounding_box.min.y + y;

                    if gx >= 0 && gx < view.width() as i32 && gy >= 0 && gy < view.height() as i32 {
                        let pixel = view.get_pixel_mut(gx as u32, gy as u32);
                        *pixel = Rgb([
                            ((1.0 - intensity) * pixel[0] as f32 + intensity * color[0] as f32)
                                as u8,
                            ((1.0 - intensity) * pixel[1] as f32 + intensity * color[1] as f32)
                                as u8,
                            ((1.0 - intensity) * pixel[2] as f32 + intensity * color[2] as f32)
                                as u8,
                        ]);
                    }
                });
            }
        }
    }
    view
}
