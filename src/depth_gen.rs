use crate::image_types::{DepthImage, TextureImage};
use image::ImageBuffer;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::rc::Rc;
use tungstenite::{connect, Message};
use ureq_multipart::MultipartBuilder;
use url::Url;

use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub struct DepthConfig {
    pub comfy_url: String,
    pub cache_dir: Option<PathBuf>,
}

fn create_cache_key(input_path: &Path, config: &DepthConfig) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();

    // Hash the input file contents
    let input_contents = fs::read(input_path)?;
    hasher.update(&input_contents);

    // Hash relevant config settings that affect the output
    hasher.update(config.comfy_url.as_bytes());

    let result = format!("{:x}", hasher.finalize());
    Ok(result)
}

type TextDispatchFn<'a> = Box<dyn Fn(&str) + 'a>;
type BinaryDispatchFn<'a> = Box<dyn Fn(&[u8]) -> Result<(), Box<dyn Error>> + 'a>;

struct WsMessageHandler<'a> {
    current_node: String,
    node_dispatch_text: HashMap<String, TextDispatchFn<'a>>,
    node_dispatch_binary: HashMap<String, BinaryDispatchFn<'a>>,
}

impl<'a> WsMessageHandler<'a> {
    fn handle_ws_message(&mut self, msg: Message) -> Result<bool, Box<dyn Error>> {
        match msg {
            Message::Text(text) => {
                let data: Value = serde_json::from_str(&text)?;
                if data["type"] == "executing" {
                    if let Some(node) = data["data"]["node"].as_str() {
                        self.current_node = node.into();

                        if let Some(handler) = self.node_dispatch_text.get(&self.current_node) {
                            handler(&text);
                        }
                    } else {
                        return Ok(true); // Execution complete
                    }
                }
                Ok(false)
            }
            Message::Binary(bytes) => {
                if let Some(handler) = self.node_dispatch_binary.get(&self.current_node) {
                    handler(&bytes)?;
                }
                Ok(false)
            }
            Message::Ping(_) => Ok(false),
            Message::Pong(_) => Ok(false),
            Message::Close(_) => Ok(true),
            Message::Frame(_) => Ok(false),
        }
    }
}

fn find_node_id(workflow: &Value, class_type: &str) -> Option<String> {
    workflow
        .as_object()?
        .iter()
        .find(|(_, node)| node["class_type"] == class_type)
        .map(|(id, _)| id.to_string())
}

pub fn generate_depth(
    input_path: PathBuf,
    config: &DepthConfig,
) -> Result<(TextureImage, DepthImage), Box<dyn Error>> {
    // Create cache directory if it doesn't exist
    if let Some(cache_dir) = &config.cache_dir {
        fs::create_dir_all(cache_dir)?;

        // Generate cache key
        let cache_key = create_cache_key(&input_path, config)?;
        let cache_path = cache_dir.join(format!("{}_rgbd.png", cache_key));
        // Check if cached version exists
        if cache_path.exists() {
            log::debug!("Loading cached RGBD image from: {}", cache_path.display());
            log::debug!("Loading cached RGBD image from: {}", cache_path.display());
            let cached_image = image::open(&cache_path)?.to_rgb8();
            let width = cached_image.width();
            let half_width = width / 2;
            let height = cached_image.height();

            // Split the cached image into texture and depth components
            let mut texture = ImageBuffer::new(half_width, height);
            let mut depth = ImageBuffer::new(half_width, height);

            for y in 0..height {
                for x in 0..half_width {
                    texture.put_pixel(x, y, *cached_image.get_pixel(x, y));
                    depth.put_pixel(x, y, *cached_image.get_pixel(x + half_width, y));
                }
            }

            log::debug!("Successfully loaded cached RGBD image");
            return Ok((TextureImage(texture), DepthImage(depth)));
        }
    }

    // If not cached, generate new depth map
    log::debug!("No cached version found, generating new depth map");

    // Load the workflow template
    let workflow_str = include_str!("../data/DepthWorkflow.json");
    let mut workflow: Value = serde_json::from_str(workflow_str)?;

    use image::io::Reader as ImageReader;
    use std::fs::File;
    use std::io::BufReader;

    // Load input image with EXIF orientation
    let reader = ImageReader::open(&input_path)?;

    // Read and decode the image
    let img = reader.decode()?.to_rgb8();

    // Create EXIF reader and try to read EXIF data from file directly
    let file = File::open(&input_path)?;
    let exif_reader = exif::Reader::new();
    let mut rotated = image::DynamicImage::ImageRgb8(img.clone());
    rotated = match exif_reader.read_from_container(&mut BufReader::new(file)) {
        Ok(exif_data) => {
            match exif_data.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
                Some(orientation) => {
                    match orientation.value.get_uint(0) {
                        Some(1) => rotated,                     // Normal orientation
                        Some(2) => rotated.fliph(),             // Mirrored horizontally
                        Some(3) => rotated.rotate180(),         // Rotated 180 degrees
                        Some(4) => rotated.flipv(),             // Mirrored vertically
                        Some(5) => rotated.fliph().rotate270(), // Mirrored horizontally and rotated 270 degrees
                        Some(6) => rotated.rotate90(),          // Rotated 90 degrees
                        Some(7) => rotated.fliph().rotate90(), // Mirrored horizontally and rotated 90 degrees
                        Some(8) => rotated.rotate270(),        // Rotated 270 degrees
                        _ => {
                            log::warn!("Unknown EXIF orientation value, defaulting to 0");
                            rotated
                        }
                    }
                }
                None => {
                    log::debug!("No EXIF orientation tag found");
                    rotated
                }
            }
        }
        Err(e) => {
            log::debug!("Failed to read EXIF data: {}", e);
            rotated
        }
    };

    // Use the rotated image instead of raw input
    let input_image = rotated.clone();

    let filename = input_path
        .file_name()
        .ok_or("input path does not contain a file name")?
        .to_string_lossy();

    // Upload image as multipart form with temp subfolder
    let (content_type, data) = MultipartBuilder::new()
        .add_file("image", &input_path)
        .unwrap()
        .add_text("subfolder", "temp")
        .unwrap()
        .finish()
        .unwrap();

    log::debug!(
        "Uploading image {} to {}/upload/image",
        filename,
        config.comfy_url
    );
    let response: Value = ureq::post(&format!("{}/upload/image", config.comfy_url))
        .set("Content-Type", &content_type)
        .send_bytes(&data)?
        .into_json()?;
    log::debug!("Upload complete");

    // Get the full path including subfolder from response
    let uploaded_path = if let Some(subfolder) = response["subfolder"].as_str() {
        format!(
            "{}/{}",
            subfolder,
            response["name"].as_str().unwrap_or(&filename)
        )
    } else {
        response["name"].as_str().unwrap_or(&filename).to_string()
    };
    log::debug!("Uploaded image path: {}", uploaded_path);

    // Update workflow with uploaded image path
    let mut load_image = workflow
        .as_object_mut()
        .unwrap()
        .into_iter()
        .filter_map(|(_, v)| {
            if v["class_type"] == "LoadImage" {
                Some(v)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    load_image[0]["inputs"]["image"] = Value::String(uploaded_path.clone());

    // Find the SaveImageWebsocket node ID
    let save_image_node_id = find_node_id(&workflow, "SaveImageWebsocket")
        .ok_or("Could not find SaveImageWebSocket node in workflow")?;

    // Queue the prompt
    let prompt_response: Value = ureq::post(&format!("{}/prompt", config.comfy_url))
        .send_json(serde_json::json!({
            "prompt": workflow,
            "client_id": "depth_charge"
        }))?
        .into_json()?;

    let prompt_id = prompt_response["prompt_id"].as_str().unwrap();
    log::debug!("Workflow queued with prompt_id: {}", prompt_id);

    // Connect to WebSocket
    let ws_url = Url::parse(&format!(
        "{}/ws?clientId=depth_charge",
        config.comfy_url.replace("http", "ws")
    ))?;
    let (mut socket, _) = connect(ws_url)?;

    // Wait for completion and image data
    let image_bytes = Rc::new(RefCell::new(None));
    {
        let save_image: Box<dyn for<'a> Fn(&'a [u8]) -> Result<(), Box<dyn Error>>> =
            Box::new(|bytes: &[u8]| -> Result<(), Box<dyn Error>> {
                // first 8 bytes are some id (1, 2) in 4 byte ints.
                *image_bytes.borrow_mut() = Some(Vec::from(&bytes[8..]));
                Ok(())
            });

        let dispatch: HashMap<String, _> = (vec![(save_image_node_id.clone(), save_image)])
            .into_iter()
            .collect();
        let mut handler = WsMessageHandler {
            current_node: "".into(),
            node_dispatch_text: HashMap::new(),
            node_dispatch_binary: dispatch,
        };

        while !handler.handle_ws_message(socket.read()?)? {}
    }

    // let input_img = image::load_from_memory(&input_image).unwrap().to_rgb8();
    let depth_img = image::load_from_memory(&image_bytes.take().expect("expected an image"))
        .unwrap()
        .to_rgb8();

    let texture = TextureImage(input_image.to_rgb8());
    let depth = DepthImage(depth_img);

    // Save to cache
    let mut cached_image = ImageBuffer::new(texture.width() * 2, texture.height());

    // Copy texture to left half
    for y in 0..texture.height() {
        for x in 0..texture.width() {
            cached_image.put_pixel(x, y, *texture.0.get_pixel(x, y));
        }
    }

    // Copy depth to right half
    for y in 0..depth.height() {
        for x in 0..depth.width() {
            cached_image.put_pixel(x + texture.width(), y, *depth.0.get_pixel(x, y));
        }
    }

    if let Some(cache_dir) = &config.cache_dir {
        let cache_key = create_cache_key(&input_path, config)?;
        let cache_path = cache_dir.join(format!("{}_rgbd.png", cache_key));
        cached_image.save(&cache_path)?;
        log::debug!("Saved RGBD image to cache: {}", cache_path.display());
    }

    Ok((texture, depth))
}
