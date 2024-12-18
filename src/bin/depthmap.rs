use clap::Parser;
use quilt_painter::image_types::{DepthImage, RgbdImage, TextureImage};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use tungstenite::{connect, Message};
use ureq_multipart::MultipartBuilder;
use url::Url;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(index = 1)]
    input: PathBuf,

    #[arg(index = 2)]
    output: String,

    #[arg(long, default_value = "http://127.0.0.1:8188")]
    comfy_url: String,
}

fn find_node_id(workflow: &Value, class_type: &str) -> Option<String> {
    workflow
        .as_object()?
        .iter()
        .find(|(_, node)| node["class_type"] == class_type)
        .map(|(id, _)| id.to_string())
}

struct WsMessageHandler<'a> {
    current_node: String,

    // Node id -> handler
    node_dispatch_text: &'a HashMap<String, Box<dyn Fn(&str) -> ()>>,
    node_dispatch_binary: &'a HashMap<String, Box<dyn Fn(&[u8]) -> ()>>,
}
impl<'a> WsMessageHandler<'a> {
    fn handle_ws_message(&mut self, msg: Message) -> Result<bool, Box<dyn std::error::Error>> {
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
                    handler(&bytes);
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = Args::parse();

    // Load the workflow template
    let workflow_str = include_str!("../../data/DepthWorkflow.json");
    let mut workflow: Value = serde_json::from_str(workflow_str)?;

    // Load input image
    let input_image = std::fs::read(&args.input)?;
    let filename = args
        .input
        .file_name()
        .ok_or("input path does not contain a file name")?
        .to_string_lossy();

    // Upload image as multipart form with temp subfolder
    let (content_type, data) = MultipartBuilder::new()
        .add_file("image", &args.input)
        .unwrap()
        .add_text("subfolder", "temp")
        .unwrap()
        .finish()
        .unwrap();

    log::debug!(
        "Uploading image {} to {}/upload/image",
        filename,
        args.comfy_url
    );
    let response: Value = ureq::post(&format!("{}/upload/image", args.comfy_url))
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

    log::debug!("Updated workflow with image name: {}", uploaded_path);
    log::debug!(
        "Workflow configuration: {}",
        serde_json::to_string_pretty(&workflow)?
    );

    // Find the SaveImageWebsocket node ID
    let save_image_node_id = find_node_id(&workflow, "SaveImageWebsocket")
        .ok_or("Could not find SaveImageWebSocket node in workflow")?;
    log::debug!("Found SaveImageWebSocket node ID: {}", save_image_node_id);

    // Queue the prompt
    log::debug!("Queueing workflow at {}/prompt", args.comfy_url);
    let prompt_response: Value = ureq::post(&format!("{}/prompt", args.comfy_url))
        .send_json(serde_json::json!({
            "prompt": workflow,
            "client_id": "depth_charge"
        }))?
        .into_json()?;

    let prompt_id = prompt_response["prompt_id"].as_str().unwrap();
    log::debug!("Workflow queued with prompt_id: {}", prompt_id);
    log::debug!(
        "Full prompt response: {}",
        serde_json::to_string_pretty(&prompt_response)?
    );

    // Connect to WebSocket
    let ws_url = Url::parse(&format!(
        "{}/ws?clientId=depth_charge",
        args.comfy_url.replace("http", "ws")
    ))?;
    let (mut socket, _) = connect(ws_url)?;

    // Wait for completion and image data
    let output_filename = args.output.clone();
    let save_image: Box<dyn Fn(&[u8]) -> ()> = Box::new(move |bytes: &[u8]| {
        // first 8 bytes are some id (1, 2) in 4 byte ints.
        let image_bytes = &bytes[8..];

        // We have the depth image, let's combine and save
        let input_img = image::load_from_memory(&input_image).unwrap().to_rgb8();
        let depth_img = image::load_from_memory(image_bytes).unwrap().to_rgb8();

        // Create and save combined RGBD image
        let rgbd = RgbdImage::from((TextureImage(input_img), DepthImage(depth_img)));
        rgbd.0.save(&output_filename).unwrap();
        println!("Saved combined RGBD image to: {}", output_filename);
    });

    let dispatch: HashMap<String, _> = (vec![(save_image_node_id.clone(), save_image)])
        .into_iter()
        .collect();
    let mut handler = WsMessageHandler {
        current_node: "".into(),
        node_dispatch_text: &HashMap::new(),
        node_dispatch_binary: &dispatch,
    };

    loop {
        match socket.read() {
            Ok(msg) => {
                // Truncate WebSocket message logging
                let debug_msg = match &msg {
                    Message::Text(text) => format!(
                        "Text({}{})",
                        text.chars().take(1000).collect::<String>(),
                        if text.len() > 1000 { "..." } else { "" }
                    ),
                    Message::Binary(bytes) => format!(
                        "Binary({} bytes): {:?}...",
                        bytes.len(),
                        bytes.iter().take(16).collect::<Vec<&u8>>()
                    ),
                    other => format!("{:?}", other),
                };
                log::debug!("Received WebSocket message: {}", debug_msg);
                if handler.handle_ws_message(msg)? {
                    break Ok(());
                }
            }
            Err(e) => {
                eprintln!("WebSocket error: {}", e);
                break Ok(());
            }
        }
    }
}
