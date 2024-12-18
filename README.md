# RGB to LookingGlass Quilt

Convert RGB images to Looking Glass quilts.

## TL;DR
1. Install [ComfyUI](https://github.com/comfyanonymous/ComfyUI) with a [DepthAnythingV2](https://github.com/kijai/ComfyUI-DepthAnythingV2) custom node.
2. Put together a bunch of images in a directory.
3. Run `cargo run --release --bin batch_depth -- --device go image_dir out_dir`
4. Copy out_dir and out_dir.m3u to your device. (On linux deploy-to-device.sh will do this)

## Installation

```bash
cargo install --path .
```

Or, if you want to render captions on the images, add a TTF as `assets/font.ttf` and
```bash
cargo install --path . --features captions
```

## Commands

This toolkit provides three main commands:

### painter

Converts an RGB+D (side-by-side RGB and depth) image to a Looking Glass quilt.

```bash
painter <input> <output> [OPTIONS]

Arguments:
  <input>    Path to input RGB+D image
  <output>   Output base name (e.g. output.png)

Options:
  -d, --device <DEVICE>    Target device (go, portrait, 16l, 16p, 32l, 32p, 65)
  --columns <COLUMNS>      Number of columns (required if device not specified)
  --rows <ROWS>           Number of rows (required if device not specified)
  --width <WIDTH>         Output width (required if device not specified)
  --height <HEIGHT>       Output height (required if device not specified)
  --fov <FOV>            Field of view in degrees [default: 60]
  --zoom <ZOOM>          Zoom factor [default: 1.0]
  --scale <SCALE>        Height enhancement [default: 1.0]
  --resize <RESIZE>      Resize multiplier [default: 2.0]
  --bg <COLOR>           Background color (black/sky/debug/RGB) [default: black]
  -L, --link-output      Create symlink from output to generated file
```

### depthmap

Generates a depth map from an RGB image using ComfyUI.

```bash
depthmap <input> <output> [OPTIONS]

Arguments:
  <input>    Path to input RGB image
  <output>   Output RGB+D image path

Options:
  --comfy-url <URL>    ComfyUI server URL [default: http://127.0.0.1:8188]
```

### depthpainter

Combined RGB to depth to quilt conversion.

```bash
depthpainter <input> <output> [OPTIONS]

Arguments:
  <input>    Path to input RGB image
  <output>   Output quilt image path

Options:
  Same as painter and depthmap combined
```

### batch_depth

Batch process a directory of images to quilts, with progress tracking and playlist generation.

```bash
batch_depth <input_dir> <output_dir> [OPTIONS]

Arguments:
  <input_dir>     Directory containing input RGB images
  <output_dir>    Directory for output quilt images

Options:
  Same as depthpainter, plus:
  --comfy-url <URL>    ComfyUI server URL [default: http://127.0.0.1:8188]
```

Features:
- Processes all images (jpg, jpeg, png) in input directory
- Tracks progress in SQLite database
- Skips already processed files
- Generates m3u playlist
- Continues from last position if interrupted

## Examples

Convert an RGB+D image to a Looking Glass Portrait quilt:
```bash
painter input_rgbd.png output.png --device portrait
```

Generate depth map from RGB image:
```bash
depthmap input.png output_rgbd.png
```

Convert RGB directly to quilt:
```bash
depthpainter input.png output.png --device portrait
```

## Debug Options

The `--debug-mode` option accepts comma-separated key=value pairs:

- `heightmap=zero` - Use flat heightmap
- `texture=heightmap` - Use heightmap as texture
- `texture=zbuffer` - Visualize z-buffer
- `startpt=<hex>` - Color start points (e.g. FF0000)
- `endpt=<hex>` - Color end points

Example:
```bash
painter input.png output.png --device portrait --debug-mode "texture=zbuffer"
```
