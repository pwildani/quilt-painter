use crate::captions::CaptionConfig;
use crate::debug::{CliDebugFlags, DebugFlags, NullDebugFlags};
use crate::image_types::{DepthImage, RgbdImage, TextureImage};
use crate::quilt::{get_quilt_settings, make_quilt, QuiltSettings};
use image::{ImageBuffer, Rgb};

pub struct QuiltConfig {
    pub device: Option<String>,
    pub columns: Option<u32>,
    pub rows: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub debug_mode: Option<String>,
    pub bg: String,
    pub fov: f32,
    pub zoom: f32,
    pub scale: f32,
    pub resize: f32,
    pub symlink_output: bool,
    pub caption: CaptionConfig,
}

pub fn parse_color(arg: &str) -> Option<Rgb<u8>> {
    match arg {
        "black" => Some(Rgb([0, 0, 0])),
        "sky" => Some(Rgb([128, (0.7 * 255.0) as u8, 255])),
        "debug" => Some(Rgb([255, 0, 255])),
        rgb => {
            if rgb.contains(',') {
                // parse 0,0,0
                let parts: Vec<u8> = rgb
                    .split(',')
                    .map(|s| s.trim().parse::<u8>().unwrap_or(0))
                    .collect();
                if parts.len() == 3 {
                    Some(Rgb([parts[0], parts[1], parts[2]]))
                } else {
                    Some(Rgb([0, 0, 0]))
                }
            } else {
                // parse hex #rrggbb or rrggbb
                let s = rgb.trim_start_matches('#');

                // Parse 6-digit hex code
                if s.len() == 6 {
                    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                    Some(Rgb([r, g, b]))
                } else {
                    None
                }
            }
        }
    }
}

pub fn generate_quilt(
    mut texture: TextureImage,
    mut heightmap: DepthImage,
    output_base_name: String,
    config: &QuiltConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    let custom_device: QuiltSettings;

    let quilt_settings = if let Some(device) = &config.device {
        get_quilt_settings(device).expect("Unknown device")
    } else {
        custom_device = QuiltSettings {
            columns: config
                .columns
                .expect("Columns must be specified for custom settings"),
            rows: config
                .rows
                .expect("Rows must be specified for custom settings"),
            resolution: (
                config
                    .width
                    .expect("Width must be specified for custom settings"),
                config
                    .height
                    .expect("Height must be specified for custom settings"),
            ),
        };
        &custom_device
    };

    // Calculate target dimensions based on tile size and resize multiplier
    let tile_width = quilt_settings.resolution.0 / quilt_settings.columns;
    let tile_height = quilt_settings.resolution.1 / quilt_settings.rows;
    let target_width = (tile_width as f32 * config.resize) as u32;
    let target_height = (tile_height as f32 * config.resize) as u32;

    // Resize if input is larger than target, preserving aspect ratio
    if texture.width() > target_width || texture.height() > target_height {
        let aspect_ratio = texture.width() as f32 / texture.height() as f32;
        let (new_width, new_height) = if target_width as f32 / target_height as f32 > aspect_ratio {
            // Height is the limiting factor
            let new_height = target_height;
            let new_width = (target_height as f32 * aspect_ratio) as u32;
            (new_width, new_height)
        } else {
            // Width is the limiting factor
            let new_width = target_width;
            let new_height = (target_width as f32 / aspect_ratio) as u32;
            (new_width, new_height)
        };

        texture = TextureImage(image::imageops::resize(
            &texture.0,
            new_width,
            new_height,
            image::imageops::FilterType::Lanczos3,
        ));
        heightmap = DepthImage(image::imageops::resize(
            &heightmap.0,
            new_width,
            new_height,
            image::imageops::FilterType::Lanczos3,
        ));
    }

    let input_aspect_ratio = texture.width() as f32 / texture.height() as f32;

    let bg_color = parse_color(config.bg.as_str()).expect("valid --bg value");

    let debug_flags = if let Some(debug_str) = config.debug_mode.as_ref() {
        let mut flags = CliDebugFlags::default();
        for flag in debug_str.split(',') {
            if let Some((key, value)) = flag.split_once('=') {
                match key {
                    "heightmap" if value == "zero" => flags.zero_heightmap = true,
                    "texture" => flags.texture_mode = Some(value.to_string()),
                    "startpt" => flags.start_point_color = parse_color(value),
                    "endpt" => flags.end_point_color = parse_color(value),
                    _ => eprintln!("Unknown debug flag: {}", flag),
                }
            }
        }
        flags
    } else {
        CliDebugFlags::default()
    };

    let zero_heightmap = debug_flags.zero_heightmap();
    let texture_debug_mode = debug_flags.texture_mode();

    // If zero_heightmap is set, create a flat heightmap
    let heightmap = if zero_heightmap {
        let (width, height) = heightmap.dimensions();
        DepthImage(ImageBuffer::from_fn(width, height, |_, _| Rgb([0, 0, 0])))
    } else {
        heightmap.clone()
    };

    let texture_to_use = TextureImage(match texture_debug_mode {
        Some("heightmap") => heightmap.clone().0,
        _ => texture.0,
    });

    let quilt_image = if config.debug_mode.is_some() {
        make_quilt(
            quilt_settings,
            &texture_to_use,
            &heightmap,
            config.fov,
            config.zoom,
            config.scale,
            bg_color,
            config.caption.clone(),
            &debug_flags,
        )
    } else {
        make_quilt(
            quilt_settings,
            &texture_to_use,
            &heightmap,
            config.fov,
            config.zoom,
            config.scale,
            bg_color,
            config.caption.clone(),
            &NullDebugFlags {},
        )
    };

    // Extract extension from output_base_name or default to png
    let extension = std::path::Path::new(&output_base_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png");

    let filename = format!(
        "{}_qs{}x{}a{:.2}.{}",
        output_base_name.trim_end_matches(&format!(".{}", extension)),
        quilt_settings.columns,
        quilt_settings.rows,
        input_aspect_ratio,
        extension
    );

    quilt_image.save(&filename)?;
    println!("Saved quilt image as: {}", filename);

    // Create symlink if requested
    if config.symlink_output {
        let link_name = output_base_name;
        // Remove existing symlink if it exists
        if std::path::Path::new(&link_name).exists() {
            std::fs::remove_file(&link_name).unwrap_or_else(|e| {
                eprintln!("Warning: Failed to remove existing symlink: {}", e);
            });
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(&filename, &link_name).unwrap_or_else(|e| {
            eprintln!("Warning: Failed to create symlink: {}", e);
        });

        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&filename, &link_name).unwrap_or_else(|e| {
            eprintln!("Warning: Failed to create symlink: {}", e);
        });

        println!("Created symlink: {} -> {}", link_name, filename);
    }

    Ok(filename)
}

pub fn split_rgbd_image(img: ImageBuffer<Rgb<u8>, Vec<u8>>) -> (TextureImage, DepthImage) {
    RgbdImage(img).split()
}

pub fn load_rgbd_image(path: &str) -> (TextureImage, DepthImage) {
    let img = image::open(path).unwrap().to_rgb8();
    split_rgbd_image(img)
}
