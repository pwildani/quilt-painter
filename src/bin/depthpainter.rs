use clap::Parser;
use quilt_painter::captions::CaptionConfig;
use quilt_painter::depth_gen::{generate_depth, DepthConfig};
use quilt_painter::quilt_gen::{generate_quilt, QuiltConfig};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(index = 1)]
    input: PathBuf,

    #[arg(index = 2)]
    output: String,

    #[arg(long, default_value = "http://127.0.0.1:8188")]
    comfy_url: String,

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
        help = "Comma separated key=value pairs for debug options",
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

    #[arg(long, default_value = "1.05", help = "zoom towards center of image")]
    zoom: f32,

    #[arg(long, default_value = "1.0", help = "enhance height")]
    scale: f32,

    #[arg(
        long,
        default_value = "2.5",
        help = "resize multiplier relative to tile size. Currently affects rendered height."
    )]
    resize: f32,

    #[arg(short = 'L', long = "link-output", alias = "link_output")]
    symlink_output: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = Args::parse();

    // Generate depth map first
    let (texture, depth) = generate_depth(
        args.input.clone(),
        &DepthConfig {
            comfy_url: args.comfy_url,
            cache_dir: None,
        },
    )?;

    // Then generate quilt
    generate_quilt(
        texture,
        depth,
        args.output,
        &QuiltConfig {
            device: args.device,
            columns: args.columns,
            rows: args.rows,
            width: args.width,
            height: args.height,
            debug_mode: args.debug_mode,
            bg: args.bg,
            fov: args.fov,
            zoom: args.zoom,
            scale: args.scale,
            resize: args.resize,
            symlink_output: args.symlink_output,
            caption: CaptionConfig::default(),
        },
    )?;

    Ok(())
}
