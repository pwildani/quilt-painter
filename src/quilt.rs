use crate::{
    camera::{self, Camera},
    captions::{draw_caption, CaptionConfig},
    debug::DebugFlags,
    image_types::{DepthImage, TextureImage},
};
use image::Pixel;
use image::{ImageBuffer, Rgb};
use itertools::Itertools;
use lazy_static::lazy_static;
use nalgebra as na;
use rayon::prelude::*;

fn ease_in_out(t: f32, w1: f32, w2: f32) -> f32 {
    // quadratic bezier
    let b2 = |t: f32, p0: f32, p1: f32, p2: f32| -> f32 {
        (1.0 - t) * ((1.0 - t) * p0 + t * p1 + t * ((1.0 - t) * p1 + t * p2))
    };

    // Cubic bezier
    (1.0 - t) * b2(t, 0.0, w1, w2) + t * b2(t, w1, w2, 1.0)
}

fn rgb_to_lum(rgb: Rgb<u8>) -> f32 {
    (0.2126 * rgb[0] as f32 + 0.7152 * rgb[1] as f32 + 0.0722 * rgb[2] as f32) / 255.0
}

#[derive(Clone, Copy, Default)]
pub struct QuiltSettings {
    pub columns: u32,
    pub rows: u32,
    pub resolution: (u32, u32),
}

lazy_static! {
    pub static ref QUILT_SETTINGS: std::collections::HashMap<&'static str, QuiltSettings> = {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "Looking Glass Go",
            QuiltSettings {
                columns: 10,
                rows: 6,
                resolution: (4092, 4092),
            },
        );
        m.insert(
            "go",
            QuiltSettings {
                columns: 10,
                rows: 6,
                resolution: (4092, 4092),
            },
        );
        m.insert(
            "Looking Glass Portrait",
            QuiltSettings {
                columns: 8,
                rows: 6,
                resolution: (3360, 3360),
            },
        );
        m.insert(
            "portrait",
            QuiltSettings {
                columns: 8,
                rows: 6,
                resolution: (3360, 3360),
            },
        );
        m.insert(
            "Looking Glass 16\" Landscape",
            QuiltSettings {
                columns: 7,
                rows: 7,
                resolution: (5999, 5999),
            },
        );
        m.insert(
            "16l",
            QuiltSettings {
                columns: 7,
                rows: 7,
                resolution: (5999, 5999),
            },
        );
        m.insert(
            "Looking Glass 16\" Portrait",
            QuiltSettings {
                columns: 11,
                rows: 6,
                resolution: (5995, 6000),
            },
        );
        m.insert(
            "16p",
            QuiltSettings {
                columns: 11,
                rows: 6,
                resolution: (5995, 6000),
            },
        );
        m.insert(
            "Looking Glass 32\" Landscape",
            QuiltSettings {
                columns: 7,
                rows: 7,
                resolution: (8190, 8190),
            },
        );
        m.insert(
            "32l",
            QuiltSettings {
                columns: 7,
                rows: 7,
                resolution: (8190, 8190),
            },
        );
        m.insert(
            "Looking Glass 32\" Portrait",
            QuiltSettings {
                columns: 11,
                rows: 6,
                resolution: (8184, 8184),
            },
        );
        m.insert(
            "32p",
            QuiltSettings {
                columns: 11,
                rows: 6,
                resolution: (8184, 8184),
            },
        );
        m.insert(
            "Looking Glass 65\"",
            QuiltSettings {
                columns: 8,
                rows: 9,
                resolution: (8192, 8192),
            },
        );
        m.insert(
            "65",
            QuiltSettings {
                columns: 8,
                rows: 9,
                resolution: (8192, 8192),
            },
        );
        m
    };
}

pub fn get_quilt_settings(device: &str) -> Option<&'static QuiltSettings> {
    QUILT_SETTINGS.get(device)
}

/// Creates a quilt image from the input texture and heightmap
///
/// # Arguments
/// * `settings` - The quilt settings for the target device
/// * `texture` - The RGB texture image
/// * `heightmap` - The grayscale heightmap image
/// * `fov_deg` - Field of view in degrees
/// * `zoom` - Zoom factor
/// * `scale` - Height scale factor
/// * `bg_color` - Background color
/// * `debug_kv` - Debug key-value pairs
///
/// # Returns
/// The generated quilt image
pub fn make_quilt<D: DebugFlags>(
    settings: &QuiltSettings,
    texture: &TextureImage,
    heightmap: &DepthImage,
    fov_deg: f32,
    zoom: f32,
    scale: f32,
    bg_color: Rgb<u8>,
    caption: CaptionConfig,
    debug_flags: &D,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let quilt_views = render_quilt_views(
        settings.resolution.0,
        settings.resolution.1,
        settings.columns,
        settings.rows,
        texture,
        heightmap,
        zoom,
        fov_deg,
        scale,
        bg_color,
        debug_flags,
        caption,
    );
    stitch_quilt(&quilt_views, settings.columns, settings.rows)
}

/// Renders all views for the quilt
///
/// # Arguments
/// * `quilt_width` - Width of the final quilt
/// * `quilt_height` - Height of the final quilt  
/// * `columns` - Number of columns in the quilt
/// * `rows` - Number of rows in the quilt
/// * `texture` - The RGB texture image
/// * `heightmap` - The grayscale heightmap image
/// * `zoom` - Zoom factor
/// * `fov_deg` - Field of view in degrees
/// * `scale` - Height scale factor
/// * `bg_color` - Background color
/// * `debug_kv` - Debug key-value pairs
///
/// # Returns
/// Vector of rendered view images
fn render_quilt_views<D: DebugFlags>(
    quilt_width: u32,
    quilt_height: u32,
    columns: u32,
    rows: u32,
    texture: &TextureImage,
    heightmap: &DepthImage,
    zoom: f32,
    fov_deg: f32,
    scale: f32,
    bg_color: Rgb<u8>,
    debug_flags: &D,
    caption: CaptionConfig,
) -> Vec<ImageBuffer<Rgb<u8>, Vec<u8>>> {
    let num_views = columns * rows;
    let view_width = quilt_width / columns;
    let view_height = quilt_height / rows;

    // fov is centered at origin.
    let fov_size = fov_deg / 360.0 * std::f32::consts::PI;
    let fov_low = -fov_size / 2.0;

    // Parallize over each view point. The smallest unit of parallelization we could do without
    // address conflicts should be a single y-line of an output image (not a input texture row) ,
    // but the image crate doesn't offer a way to slice out chunks of image like that, so lazily we
    // just do whole images.
    (0..num_views)
        .into_par_iter()
        .map(|i| {
            let view_theta = fov_size * i as f32 / (num_views - 1) as f32 + fov_low;
            log::debug!(
                "Camera theta degrees: {:?}",
                view_theta / std::f32::consts::PI * 360.0
            );
            let camera = Camera {
                zoom,
                view_width,
                view_height,
                view_theta,
                z_scale: scale,
            };
            let rotation = na::UnitComplex::from_angle(view_theta);
            let view = render_view(texture, heightmap, camera, rotation, bg_color, debug_flags);
            let view = draw_caption(view, caption.clone());
            view
        })
        .collect()
}

/// Stitches individual view images into the final quilt
///
/// # Arguments
/// * `views` - Vector of rendered view images
/// * `columns` - Number of columns in the quilt
/// * `rows` - Number of rows in the quilt
///
/// # Returns
/// The final stitched quilt image
fn stitch_quilt(
    views: &[ImageBuffer<Rgb<u8>, Vec<u8>>],
    columns: u32,
    rows: u32,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let (view_width, view_height) = views[0].dimensions();
    let quilt_width = view_width * columns;
    let quilt_height = view_height * rows;
    let mut quilt = ImageBuffer::new(quilt_width, quilt_height);

    for (i, view) in views.iter().enumerate() {
        let row = i as u32 / columns;
        let col = columns - (i as u32 % columns) - 1;
        let y_start = row * view_height;
        let x_start = col * view_width;

        for (x, y, pixel) in view.enumerate_pixels() {
            quilt.put_pixel(x_start + x, y_start + y, *pixel);
        }
    }

    quilt
}

#[derive(Debug, Clone, Copy)]
struct PrevRender {
    x: u32,
    z: f32,
    color: Rgb<u8>,
}

fn render_px<D: DebugFlags>(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    texture: &TextureImage,
    camera: &camera::Camera,
    rot: &na::UnitComplex<f32>,
    tex_y: u32,
    tex_x: u32,
    screen_y: u32,
    height: f32,
    zbuffer: &mut na::DMatrix<f32>,
    prev: Option<PrevRender>,
    debug_flags: &D,
) -> Option<PrevRender> {
    let (tex_width, _tex_height) = texture.dimensions();
    let x_img = tex_x as f32 - (tex_width as f32) / 2.0;
    // let screen_x_0 = camera.view_width as f32 / 2.0;

    let z0 = 0.0;
    let color = *texture.0.get_pixel(tex_x, tex_y);

    // We want to draw a line along the normal from the surface at (x,y,z0) (start_pt) to the displaced
    // height(x,y,z0+height). The surface is rotated by camera.rot around the y axis
    let pt = rot * na::point!(z0 + (height) * camera.z_scale, x_img);
    const EPSILON: f32 = 1e-5;

    let screen_x = (pt[1] * camera.zoom * (camera.view_width as f32 / tex_width as f32)
        + camera.view_width as f32 / 2.0)
        .round();

    if screen_x < 0.0 {
        return None;
    }

    if screen_x >= 0.0
        && screen_x < camera.view_width as f32
        && pt[0] > zbuffer[(screen_x as usize, screen_y as usize)]
    {
        zbuffer[(screen_x as usize, screen_y as usize)] = pt[0];
        img.put_pixel(screen_x as u32, screen_y, color);
    }

    // Draw gradient from last
    if let Some(prev) = prev {
        let (start, start_z, start_color, end, end_z, end_color) = if prev.x > screen_x as u32 {
            (prev.x, prev.z, prev.color, screen_x as u32, pt[0], color)
        } else {
            (screen_x as u32, pt[0], color, prev.x, prev.z, prev.color)
        };

        // Ensure we draw at least one pixel even if points are close
        let len = (end as i32 - start as i32).abs();
        if len >= 2 {
            if len > 1 {
                if let Some(start_color) = debug_flags.start_point_color() {
                    if start < camera.view_width && screen_y < camera.view_height {
                        img.put_pixel(start, screen_y, start_color);
                    }
                }
                if let Some(end_color) = debug_flags.end_point_color() {
                    if start < camera.view_width && screen_y < camera.view_height {
                        img.put_pixel(end, screen_y, end_color);
                    }
                }
            }
            let min_x = start.min(end);
            let max_x = start.max(end);
            let start_color_luminosity = rgb_to_lum(start_color);
            let end_color_luminosity = rgb_to_lum(end_color);
            let sharpness = 0.3333;
            let mut w1 = start_color_luminosity / (start_color_luminosity + end_color_luminosity);
            let mut w2 =
                1.0 - (end_color_luminosity / (start_color_luminosity + end_color_luminosity));
            if start_color_luminosity > end_color_luminosity {
                w2 *= sharpness;
            } else {
                w1 *= sharpness;
            }
            for draw_x in min_x..=max_x {
                // Add epsilon to avoid floating point rounding errors
                let raw_t =
                    ((draw_x as f32 - start as f32) / (len as f32 + EPSILON)).clamp(0.0, 1.0);
                let eased_t = ease_in_out(raw_t, w1, w2);
                if draw_x < camera.view_width && screen_y < camera.view_height {
                    let pt_color = start_color.map2(&end_color, |s, e| {
                        ((e as f32 - s as f32) * eased_t + s as f32).clamp(0.0, 255.0) as u8
                    });
                    let z = start_z + (end_z - start_z) * raw_t;
                    if z > zbuffer[(draw_x as usize, screen_y as usize)] {
                        img.put_pixel(draw_x, screen_y, pt_color);
                        zbuffer[(draw_x as usize, screen_y as usize)] = z;
                    }
                }
            }
        }
    }

    Some(PrevRender {
        x: screen_x.round() as u32,
        z: pt[0],
        color,
    })
}

/// Renders a single view from the given camera angle
fn render_view<D: DebugFlags>(
    texture: &TextureImage,
    heightmap: &DepthImage,
    camera: Camera,
    scene_rotation: na::UnitComplex<f32>,
    bg_color: Rgb<u8>,
    debug_flags: &D,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let (tex_width, tex_height) = texture.dimensions();

    let mut img = ImageBuffer::from_pixel(camera.view_width, camera.view_height, bg_color);
    let mut zbuffer: na::DMatrix<f32> = na::DMatrix::from_element(
        camera.view_width as usize,
        camera.view_height as usize,
        f32::NEG_INFINITY,
    );

    // Iterate over output image rows
    for screen_y in 0..camera.view_height {
        // Calculate texture y range that could map to this screen y
        // Zoom the y around the center of the view.
        let zoomed_screen_y = (screen_y as f32 - (camera.view_height as f32 / 2.0)) / camera.zoom;
        let zoomed_screen_y_next = zoomed_screen_y + camera.zoom;
        let tex_y_f = zoomed_screen_y * tex_height as f32 / camera.view_height as f32
            + tex_height as f32 / 2.0;
        let tex_y_next_f = (zoomed_screen_y_next) * tex_height as f32 / camera.view_height as f32
            + tex_height as f32 / 2.0;

        let tex_y_start = tex_y_f.floor() as u32;
        let tex_y_end = tex_y_next_f.ceil() as u32;

        // Process each texture y that maps to this screen y
        for tex_y in tex_y_start..=tex_y_end.min(tex_height - 1) {
            let mut last = None;
            if camera.view_theta < 0.0 {
                for tex_x in 0..tex_width {
                    let height_pixel = heightmap.0.get_pixel(tex_x, tex_y);
                    last = render_px(
                        &mut img,
                        texture,
                        &camera,
                        &scene_rotation,
                        tex_y,
                        tex_x,
                        screen_y,
                        height_pixel[0] as f32,
                        &mut zbuffer,
                        last,
                        debug_flags,
                    )
                }
            } else {
                for tex_x in (0..tex_width).rev() {
                    let height_pixel = heightmap.0.get_pixel(tex_x, tex_y);
                    last = render_px(
                        &mut img,
                        texture,
                        &camera,
                        &scene_rotation,
                        tex_y,
                        tex_x,
                        screen_y,
                        height_pixel[0] as f32,
                        &mut zbuffer,
                        last,
                        debug_flags,
                    )
                }
            }
        }
    }

    // If texture=zbuffer debug mode is on, replace the output with zbuffer visualization
    if debug_flags.texture_mode() == Some("zbuffer") {
        // Create new image for zbuffer visualization
        let mut zbuffer_img = ImageBuffer::new(camera.view_width, camera.view_height);

        // Find min/max z values for normalization
        let (min_z, max_z) = zbuffer
            .iter()
            .filter(|z| **z != f32::NEG_INFINITY)
            .minmax()
            .into_option()
            .unwrap();

        // Normalize and visualize zbuffer
        for y in 0..camera.view_height {
            for x in 0..camera.view_width {
                let z = zbuffer[(x as usize, y as usize)];
                if z == f32::NEG_INFINITY {
                    zbuffer_img.put_pixel(x, y, Rgb([0, 0, 0]));
                } else {
                    let normalized = ((z - min_z) / (max_z - min_z) * 255.0) as u8;
                    zbuffer_img.put_pixel(x, y, Rgb([normalized, normalized, normalized]));
                }
            }
        }
        zbuffer_img
    } else {
        img
    }
}
