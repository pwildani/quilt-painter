#[derive(Debug, Copy, Clone)]
pub struct Camera {
    pub zoom: f32,
    pub view_width: u32,
    pub view_height: u32,
    pub view_theta: f32,
    pub z_scale: f32,
}
