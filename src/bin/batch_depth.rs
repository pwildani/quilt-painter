use clap::Parser;
use quilt_painter::captions::CaptionConfig;
#[cfg(feature = "captions")]
use quilt_painter::captions::Position;
use quilt_painter::depth_gen::{generate_depth, DepthConfig};
use quilt_painter::quilt_gen::{generate_quilt, QuiltConfig};
use rusqlite::{Connection, Result as SqlResult};
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(index = 1)]
    input_dir: PathBuf,

    #[arg(index = 2)]
    output_dir: PathBuf,

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

fn init_db(conn: &Connection) -> SqlResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS processed_files (
            path TEXT PRIMARY KEY,
            basename TEXT,
            quiltfilename TEXT,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            status TEXT
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS playlist (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT NOT NULL REFERENCES processed_files(path),
            position INTEGER NOT NULL,
            UNIQUE(position)
        )",
        [],
    )?;
    Ok(())
}

fn get_playlist(conn: &Connection) -> SqlResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT position, quiltfilename FROM playlist JOIN processed_files ON playlist.path = processed_files.path ORDER BY position")?;
    let playlist = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<SqlResult<Vec<_>>>()?;
    Ok(playlist)
}

fn generate_nonunique_simple_name(original_name: &str) -> String {
    // Get base name without extension
    let stem = Path::new(original_name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();

    // Simplify to alphanumeric only and limit length
    stem.chars()
        .filter(|c| c.is_alphanumeric())
        .take(32)
        .collect()
}

fn generate_simple_name(conn: &Connection, original_name: &str) -> Result<String, Box<dyn Error>> {
    let simple = generate_nonunique_simple_name(original_name);

    // Check if this name exists
    let count: i32 = conn.query_row(
        "SELECT COUNT(*) FROM processed_files WHERE basename LIKE ?1",
        [format!("{simple}%")],
        |row| row.get(0),
    )?;

    // Add number suffix if needed
    let final_name = if count > 0 {
        format!("{simple}_{count:02}")
    } else {
        simple
    };

    Ok(final_name)
}

fn export_m3u_playlist(conn: &Connection, output_dir: &Path) -> Result<(), Box<dyn Error>> {
    let playlist = get_playlist(conn)?;
    // Create m3u file named for the directory name
    let dir_name = output_dir.file_name().unwrap_or_default().to_string_lossy();
    let out = output_dir.parent().unwrap_or(output_dir);
    let m3u_path = out.join(format!("{dir_name}.m3u"));
    let mut file = std::fs::File::create(m3u_path)?;

    // Write m3u header. Nope. Lookingglass Go does notaccept it.
    // writeln!(file, "#EXTM3U")?;

    // Write each entry - the path is already the simplified output filename
    for (_, filename) in playlist {
        writeln!(file, "{filename}")?;
    }

    Ok(())
}

fn add_to_playlist(conn: &Connection, path: &str) -> Result<(), Box<dyn Error>> {
    // Get the next available position
    let next_pos: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist",
        [],
        |row| row.get(0),
    )?;

    conn.execute(
        "INSERT INTO playlist (path, position) VALUES (?1, ?2)",
        (path, next_pos),
    )?;

    Ok(())
}

fn get_processing_status(conn: &Connection, path: &str) -> ProcessingStatus {
    match conn.query_row(
        "SELECT status FROM processed_files WHERE path = ?1",
        [path],
        |row| row.get::<_, String>(0),
    ) {
        Ok(status) => {
            if status == "success" {
                ProcessingStatus::Processed
            } else {
                ProcessingStatus::NeedsReprocessing
            }
        }
        Err(_) => ProcessingStatus::NotProcessed,
    }
}

#[derive(PartialEq)]
enum ProcessingStatus {
    Processed,
    NeedsReprocessing,
    NotProcessed,
}

fn mark_processed(
    conn: &Connection,
    path: &str,
    basename: &str,
    quiltfilename: &str,
    status: &str,
) -> SqlResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO processed_files (path, basename, quiltfilename, status) VALUES (?1, ?2, ?3, ?4)",
        (path, basename, quiltfilename, status),
    )?;
    Ok(())
}

fn process_image(
    input_path: &Path,
    output_dir: &Path,
    config: &DepthConfig,
    quilt_config: &QuiltConfig,
    conn: &Connection,
    caption_config: &CaptionConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get both the original filename and a simple name for the database
    let input_name = input_path.file_name().unwrap().to_string_lossy();
    let simple_name = generate_simple_name(conn, &input_name)?;

    match get_processing_status(conn, &input_path.to_string_lossy()) {
        ProcessingStatus::Processed => {
            println!("Skipping already processed file: {simple_name}");
            return Ok(());
        }
        ProcessingStatus::NeedsReprocessing => {
            println!("Reprocessing: {simple_name}");
        }
        ProcessingStatus::NotProcessed => {
            println!("Processing new file: {input_name} -> {simple_name}");
        }
    }

    println!("Processing: {simple_name}");

    let (texture, depth) = generate_depth(input_path.to_path_buf(), config)?;

    let ext = input_path
        .extension()
        .unwrap_or_else(|| std::ffi::OsStr::new("jpg"));
    let output_path = output_dir.join(&simple_name).with_extension(ext);

    // Replace {} in caption with filename if present
    #[cfg(feature = "captions")]
    let mut caption = caption_config.clone();
    #[cfg(not(feature = "captions"))]
    let caption = caption_config;
    #[cfg(feature = "captions")]
    if let Some(text) = caption.text.as_ref() {
        let base_name = input_path.file_stem().unwrap_or_default().to_string_lossy();
        caption.text = Some(text.replace("{}", &base_name));
    }

    let quiltfilename = generate_quilt(
        texture,
        depth,
        output_path.to_string_lossy().to_string(),
        &QuiltConfig {
            device: quilt_config.device.clone(),
            columns: quilt_config.columns,
            rows: quilt_config.rows,
            width: quilt_config.width,
            height: quilt_config.height,
            debug_mode: quilt_config.debug_mode.clone(),
            bg: quilt_config.bg.clone(),
            fov: quilt_config.fov,
            zoom: quilt_config.zoom,
            scale: quilt_config.scale,
            resize: quilt_config.resize,
            symlink_output: quilt_config.symlink_output,
            caption: caption.clone(),
        },
    )?;

    mark_processed(conn, &input_name, &simple_name, &quiltfilename, "success")?;
    add_to_playlist(conn, &input_name)?;
    println!("Successfully processed: {simple_name}");

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = Args::parse();

    // Create output directory if it doesn't exist
    std::fs::create_dir_all(&args.output_dir)?;

    // Initialize database
    let db_path = args.input_dir.join("index.db");
    let conn = Connection::open(db_path)?;
    init_db(&conn)?;

    // Create cache directory in input dir
    let cache_dir = args.input_dir.join(".rgbd_cache");
    let depth_config = DepthConfig {
        comfy_url: args.comfy_url.clone(),
        cache_dir: Some(cache_dir),
    };

    #[cfg(feature = "captions")]
    let caption = CaptionConfig::new(args.caption, args.caption_size, args.caption_position);
    #[cfg(not(feature = "captions"))]
    let caption = CaptionConfig::default();

    let quilt_config = QuiltConfig {
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
        symlink_output: false,
        caption: CaptionConfig::default(),
    };

    // Process all images in input directory
    for entry in WalkDir::new(&args.input_dir)
        .follow_links(true)
        .into_iter()
        .filter(|e| {
            e.as_ref().is_ok_and(|v| {
                !v.path()
                    .components()
                    .any(|c| c.as_os_str() == ".rgbd_cache")
            })
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_ascii_lowercase();
                if ext_str == "jpg" || ext_str == "jpeg" || ext_str == "png" {
                    if let Err(e) = process_image(
                        path,
                        &args.output_dir,
                        &depth_config,
                        &quilt_config,
                        &conn,
                        &caption,
                    ) {
                        let simple_name = generate_nonunique_simple_name(&path.to_string_lossy());
                        eprintln!("Error processing {}: {e}", path.display());
                        mark_processed(&conn, &path.to_string_lossy(), &simple_name, "", "error")?;
                    }
                }
            }
        }
    }

    // Export updated playlist
    export_m3u_playlist(&conn, &args.output_dir)?;
    Ok(())
}
