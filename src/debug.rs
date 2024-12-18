use image::Rgb;

pub trait DebugFlags: Send + Sync {
    fn zero_heightmap(&self) -> bool;
    fn texture_mode(&self) -> Option<&str>;
    fn start_point_color(&self) -> Option<Rgb<u8>>;
    fn end_point_color(&self) -> Option<Rgb<u8>>;
}

#[derive(Default)]
pub struct CliDebugFlags {
    pub zero_heightmap: bool,
    pub texture_mode: Option<String>,
    pub start_point_color: Option<Rgb<u8>>,
    pub end_point_color: Option<Rgb<u8>>,
}

impl DebugFlags for CliDebugFlags {
    fn zero_heightmap(&self) -> bool {
        self.zero_heightmap
    }

    fn texture_mode(&self) -> Option<&str> {
        self.texture_mode.as_deref()
    }

    fn start_point_color(&self) -> Option<Rgb<u8>> {
        self.start_point_color
    }

    fn end_point_color(&self) -> Option<Rgb<u8>> {
        self.end_point_color
    }
}

#[derive(Default)]
pub struct NullDebugFlags;

impl DebugFlags for NullDebugFlags {
    fn zero_heightmap(&self) -> bool {
        false
    }

    fn texture_mode(&self) -> Option<&str> {
        None
    }

    fn start_point_color(&self) -> Option<Rgb<u8>> {
        None
    }

    fn end_point_color(&self) -> Option<Rgb<u8>> {
        None
    }
}
