use std::{fs, path::PathBuf};

use helioframe_video::{stitch_tiles, FrameTile, StitchPlan};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct StitchConfigFile {
    stitch: StitchBaseline,
}

#[derive(Debug, Deserialize)]
struct StitchBaseline {
    frame_width: usize,
    frame_height: usize,
    channels: usize,
    tile_width: usize,
    tile_height: usize,
    overlap: usize,
    max_mean_abs_error: f32,
    max_seam_delta: f32,
}

#[test]
fn stitched_gradient_tiles_stay_within_seam_baseline() {
    let config = load_stitch_baseline();
    let truth = gradient_frame(config.frame_width, config.frame_height, config.channels);

    let tiles = tiled_samples(&truth, &config);

    let stitched = stitch_tiles(
        &tiles,
        &StitchPlan {
            frame_width: config.frame_width,
            frame_height: config.frame_height,
            channels: config.channels,
            overlap: config.overlap,
            seam_debug: true,
        },
    )
    .expect("stitch should succeed");

    let mae = mean_abs_error(&truth, &stitched.pixels);
    assert!(
        mae <= config.max_mean_abs_error,
        "mean absolute error too high: {mae} > {}",
        config.max_mean_abs_error
    );

    let seam_delta = seam_max_delta(
        &stitched.pixels,
        config.frame_width,
        config.frame_height,
        config.channels,
        config.tile_width - config.overlap,
    );
    assert!(
        seam_delta <= config.max_seam_delta,
        "seam delta too high: {seam_delta} > {}",
        config.max_seam_delta
    );

    let debug = stitched
        .seam_debug_pixels
        .expect("seam debug output should be generated");
    let overlap_probe = ((config.frame_height / 2) * config.frame_width + (config.tile_width - 2))
        * config.channels;
    assert!(
        debug[overlap_probe] > stitched.pixels[overlap_probe],
        "seam overlay should emphasize overlap in red"
    );
}

fn load_stitch_baseline() -> StitchBaseline {
    let path = repo_root()
        .join("tests")
        .join("integration")
        .join("stitch_expectations.json");
    let text = fs::read_to_string(path).expect("stitch baseline json should be readable");
    let parsed: StitchConfigFile =
        serde_json::from_str(&text).expect("stitch baseline json should parse");
    parsed.stitch
}

fn tiled_samples(truth: &[u8], cfg: &StitchBaseline) -> Vec<FrameTile> {
    let step_x = cfg.tile_width.saturating_sub(cfg.overlap).max(1);
    let step_y = cfg.tile_height.saturating_sub(cfg.overlap).max(1);
    let mut tiles = Vec::new();

    let mut y = 0;
    while y < cfg.frame_height {
        let tile_h = cfg.tile_height.min(cfg.frame_height - y);
        let mut x = 0;
        while x < cfg.frame_width {
            let tile_w = cfg.tile_width.min(cfg.frame_width - x);
            let bias = (((x / step_x) + (y / step_y)) as i16 % 3 - 1) * 7;
            let pixels = extract_tile_with_bias(
                truth,
                cfg.frame_width,
                cfg.channels,
                x,
                y,
                tile_w,
                tile_h,
                bias,
            );
            tiles.push(FrameTile {
                x,
                y,
                width: tile_w,
                height: tile_h,
                pixels,
            });
            if x + tile_w >= cfg.frame_width {
                break;
            }
            x += step_x;
        }
        if y + tile_h >= cfg.frame_height {
            break;
        }
        y += step_y;
    }

    tiles
}

fn extract_tile_with_bias(
    frame: &[u8],
    frame_width: usize,
    channels: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    bias: i16,
) -> Vec<u8> {
    let mut pixels = vec![0u8; width * height * channels];
    for local_y in 0..height {
        for local_x in 0..width {
            let src_base = ((y + local_y) * frame_width + (x + local_x)) * channels;
            let dst_base = (local_y * width + local_x) * channels;
            for c in 0..channels {
                let v = i16::from(frame[src_base + c]) + bias;
                pixels[dst_base + c] = v.clamp(0, 255) as u8;
            }
        }
    }
    pixels
}

fn gradient_frame(width: usize, height: usize, channels: usize) -> Vec<u8> {
    let mut frame = vec![0u8; width * height * channels];
    for y in 0..height {
        for x in 0..width {
            let base = (y * width + x) * channels;
            let fx = x as f32 / (width.saturating_sub(1).max(1)) as f32;
            let fy = y as f32 / (height.saturating_sub(1).max(1)) as f32;
            frame[base] = (fx * 255.0).round() as u8;
            frame[base + 1] = (fy * 255.0).round() as u8;
            frame[base + 2] = ((1.0 - fx * 0.6 - fy * 0.4).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    frame
}

fn mean_abs_error(a: &[u8], b: &[u8]) -> f32 {
    assert_eq!(a.len(), b.len());
    let total = a
        .iter()
        .zip(b)
        .map(|(left, right)| (i16::from(*left) - i16::from(*right)).abs() as f32)
        .sum::<f32>();
    total / a.len() as f32
}

fn seam_max_delta(
    frame: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    seam_x: usize,
) -> f32 {
    let left_x = seam_x.saturating_sub(1).min(width.saturating_sub(1));
    let right_x = seam_x.min(width.saturating_sub(1));
    let mut max_delta = 0f32;

    for y in 0..height {
        let left_base = (y * width + left_x) * channels;
        let right_base = (y * width + right_x) * channels;
        let mut delta = 0f32;
        for c in 0..channels {
            delta += (f32::from(frame[left_base + c]) - f32::from(frame[right_base + c])).abs();
        }
        max_delta = max_delta.max(delta / channels as f32);
    }

    max_delta
}

fn repo_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root should canonicalize")
}
