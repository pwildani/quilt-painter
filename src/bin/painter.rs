use clap::Parser;
use image::{ImageBuffer, Rgb};
use quilt_painter::captions::CaptionConfig;
#[cfg(feature = "captions")]
use quilt_painter::captions::Position;
use quilt_painter::debug::{CliDebugFlags, DebugFlags, NullDebugFlags};
use quilt_painter::image_types::{DepthImage, RgbdImage, TextureImage};
use quilt_painter::quilt::{get_quilt_settings, make_quilt, QuiltSettings};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(index = 1)]
    input: String,

    #[arg(index = 2)]
    output_base_name: String,

    #[arg(short = 'L', long = "link-output", alias = "link_output")]
    symlink_output_base_name_to_generated_name: bool,

    #[arg(short, long, conflicts_with_all=["columns", "rows", "width", "height"])]
    device: Option<String>,

    #[arg(long, help = "The number of columns of tiles in the output quilt.")]
    columns: Option<u32>,

    #[arg(long, help = "The number of rows of tiles in the output quilt.")]
    rows: Option<u32>,

    #[arg(long, help = "The width of the output quilt in pixels.")]
    width: Option<u32>,

    #[arg(long, help = "The height of the output quilt in pixels.")]
    height: Option<u32>,

    #[arg(
        long,
        help = "Comma separated key=value pairs for debug options:
        heightmap=zero - Use flat heightmap instead of input
        texture=heightmap - Use heightmap as texture
        texture=zbuffer - Visualize z-buffer instead of texture
        startpt=<hex> - Color start points with hex RGB (e.g. FF0000)
        endpt=<hex> - Color end points with hex RGB",
        alias = "debug_mode"
    )]
    debug_mode: Option<String>,

    #[arg(
        long,
        default_value = "black",
        help = "black, sky, debug or an rgb triplet"
    )]
    bg: String,

    #[arg(long, default_value = "60", help = "field of view in degrees")]
    fov: f32,

    #[arg(long, default_value = "1.0", help = "zoom towards center of image")]
    zoom: f32,

    #[arg(long, default_value = "1.0", help = "enhance height")]
    scale: f32,

    #[arg(
        long,
        default_value = "2.0",
        help = "resize multiplier relative to tile size"
    )]
    resize: f32,

    #[cfg(feature = "captions")]
    #[arg(long, help = "Optional caption text to render on the image")]
    caption: Option<String>,

    #[cfg(feature = "captions")]
    #[arg(long, default_value = "16", help = "Font size for caption in pixels")]
    caption_size: u32,

    #[cfg(feature = "captions")]
    #[arg(
        long,
        default_value = "bottom-center",
        value_enum,
        help = "Caption position (top-left, top-center, top-right, bottom-left, bottom-center)"
    )]
    caption_position: Position,

    #[cfg(not(feature = "captions"))]
    caption: (),
    #[cfg(not(feature = "captions"))]
    caption_size: (),
    #[cfg(not(feature = "captions"))]
    caption_position: (),
}

fn parse_color(arg: &str) -> Option<Rgb<u8>> {
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = Args::parse();
    let custom_device: QuiltSettings;

    let quilt_settings = if let Some(device) = &args.device {
        get_quilt_settings(device).expect("Unknown device")
    } else {
        custom_device = QuiltSettings {
            columns: args
                .columns
                .expect("Columns must be specified for custom settings"),
            rows: args
                .rows
                .expect("Rows must be specified for custom settings"),
            resolution: (
                args.width
                    .expect("Width must be specified for custom settings"),
                args.height
                    .expect("Height must be specified for custom settings"),
            ),
        };
        &custom_device
    };

    let input_img = image::open(&args.input)?;
    let (mut texture, mut heightmap) = RgbdImage(input_img.to_rgb8()).split();

    // Calculate target dimensions based on tile size and resize multiplier
    let tile_width = quilt_settings.resolution.0 / quilt_settings.columns;
    let tile_height = quilt_settings.resolution.1 / quilt_settings.rows;
    let target_width = (tile_width as f32 * args.resize) as u32;
    let target_height = (tile_height as f32 * args.resize) as u32;

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

    // Report dimensions
    println!(
        "Input image dimensions: {}x{}",
        texture.width() * 2,
        texture.height()
    );
    println!(
        "Texture dimensions: {}x{}",
        texture.width(),
        texture.height()
    );
    println!(
        "Heightmap dimensions: {}x{}",
        heightmap.width(),
        heightmap.height()
    );
    println!("Target tile dimensions: {}x{}", tile_width, tile_height);
    println!(
        "Target resize dimensions: {}x{}",
        target_width, target_height
    );

    let input_aspect_ratio = texture.width() as f32 / texture.height() as f32;

    let bg_color = parse_color(args.bg.as_str()).expect("valid --bg value");

    let debug_flags = if let Some(debug_str) = args.debug_mode.as_ref() {
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

    let texture_to_use = match texture_debug_mode {
        Some("heightmap") => TextureImage(heightmap.0.clone()),
        _ => texture,
    };

    let quilt_image = if args.debug_mode.is_some() {
        make_quilt(
            quilt_settings,
            &texture_to_use,
            &heightmap,
            args.fov,
            args.zoom,
            args.scale,
            bg_color,
            #[cfg(feature = "captions")]
            CaptionConfig::new(args.caption, args.caption_size, args.caption_position),
            #[cfg(not(feature = "captions"))]
            CaptionConfig::default(),
            &debug_flags,
        )
    } else {
        make_quilt(
            quilt_settings,
            &texture_to_use,
            &heightmap,
            args.fov,
            args.zoom,
            args.scale,
            bg_color,
            #[cfg(feature = "captions")]
            CaptionConfig::new(args.caption, args.caption_size, args.caption_position),
            #[cfg(not(feature = "captions"))]
            CaptionConfig::default(),
            &NullDebugFlags {},
        )
    };

    // Extract extension from output_base_name or default to png
    let extension = std::path::Path::new(&args.output_base_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png");

    let filename = format!(
        "{}_qs{}x{}a{:.2}.{}",
        args.output_base_name
            .trim_end_matches(&format!(".{}", extension)),
        quilt_settings.columns,
        quilt_settings.rows,
        input_aspect_ratio,
        extension
    );

    if filename.ends_with(".jpg") || filename.ends_with(".jpeg") {
        let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        comp.set_size(quilt_image.width() as usize, quilt_image.height() as usize);
        comp.set_quality(100.0);
        let mut jpeg_data = Vec::new();
        let mut comp = comp.start_compress(&mut jpeg_data)?;
        comp.write_scanlines(quilt_image.as_raw())?;
        drop(comp);
        std::fs::write(&filename, jpeg_data)?;
    } else {
        quilt_image.save(&filename)?;
    }
    println!("Saved quilt image as: {}", filename);

    // Create symlink if requested
    if args.symlink_output_base_name_to_generated_name {
        let link_name = args.output_base_name;
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    #[test]
    fn test_load_rgbd_image() {
        let mut test_image: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(4, 2);

        // Left half (texture) is red
        for x in 0..2 {
            for y in 0..2 {
                test_image.put_pixel(x, y, Rgb([255, 0, 0]));
            }
        }

        // Right half (heightmap) is gray
        for x in 2..4 {
            for y in 0..2 {
                test_image.put_pixel(x, y, Rgb([128, 128, 128]));
            }
        }

        // Save temporary image
        let temp_path = "test_rgbd.png";
        test_image
            .save(temp_path)
            .expect("Failed to save test image");

        // Test loading
        let (texture, heightmap) = load_rgbd_image(temp_path);

        assert_eq!(texture.dimensions(), (2, 2));
        assert_eq!(heightmap.dimensions(), (2, 2));

        // Check texture is red
        assert_eq!(*texture.get_pixel(0, 0), Rgb([255, 0, 0]));

        // Check heightmap is gray
        assert_eq!(*heightmap.get_pixel(0, 0), Rgb([128, 128, 128]));

        // Clean up
        std::fs::remove_file(temp_path).unwrap();
    }
}
