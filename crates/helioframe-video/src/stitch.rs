use helioframe_core::{HelioFrameError, HelioFrameResult};

#[derive(Debug, Clone)]
pub struct FrameTile {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct StitchPlan {
    pub frame_width: usize,
    pub frame_height: usize,
    pub channels: usize,
    pub overlap: usize,
    pub seam_debug: bool,
}

#[derive(Debug, Clone)]
pub struct StitchResult {
    pub pixels: Vec<u8>,
    pub seam_debug_pixels: Option<Vec<u8>>,
}

pub fn stitch_tiles(tiles: &[FrameTile], plan: &StitchPlan) -> HelioFrameResult<StitchResult> {
    if plan.frame_width == 0 || plan.frame_height == 0 {
        return Err(HelioFrameError::Config(
            "stitch frame dimensions must be non-zero".to_string(),
        ));
    }
    if plan.channels == 0 {
        return Err(HelioFrameError::Config(
            "stitch channels must be non-zero".to_string(),
        ));
    }
    if tiles.is_empty() {
        return Err(HelioFrameError::Config(
            "stitch requires at least one tile".to_string(),
        ));
    }

    let frame_pixels_len = plan
        .frame_width
        .checked_mul(plan.frame_height)
        .and_then(|px| px.checked_mul(plan.channels))
        .ok_or_else(|| HelioFrameError::Config("stitch frame pixel count overflow".to_string()))?;

    let sample_count = plan
        .frame_width
        .checked_mul(plan.frame_height)
        .ok_or_else(|| HelioFrameError::Config("stitch frame sample count overflow".to_string()))?;

    let mut accum = vec![0f32; frame_pixels_len];
    let mut weights = vec![0f32; sample_count];
    let mut coverage = vec![0u16; sample_count];

    for tile in tiles {
        validate_tile(tile, plan)?;
        accumulate_tile(tile, plan, &mut accum, &mut weights, &mut coverage);
    }

    let mut output = vec![0u8; frame_pixels_len];
    for idx in 0..sample_count {
        let w = weights[idx];
        if w <= f32::EPSILON {
            continue;
        }

        let base = idx * plan.channels;
        for c in 0..plan.channels {
            let v = (accum[base + c] / w).clamp(0.0, 255.0);
            output[base + c] = v.round() as u8;
        }
    }

    let seam_debug_pixels = if plan.seam_debug {
        Some(build_seam_debug_overlay(
            &output,
            &coverage,
            plan.frame_width,
            plan.frame_height,
            plan.channels,
        ))
    } else {
        None
    };

    Ok(StitchResult {
        pixels: output,
        seam_debug_pixels,
    })
}

fn validate_tile(tile: &FrameTile, plan: &StitchPlan) -> HelioFrameResult<()> {
    if tile.width == 0 || tile.height == 0 {
        return Err(HelioFrameError::Config(
            "tile dimensions must be non-zero".to_string(),
        ));
    }

    let x2 = tile
        .x
        .checked_add(tile.width)
        .ok_or_else(|| HelioFrameError::Config("tile x extent overflow".to_string()))?;
    let y2 = tile
        .y
        .checked_add(tile.height)
        .ok_or_else(|| HelioFrameError::Config("tile y extent overflow".to_string()))?;

    if x2 > plan.frame_width || y2 > plan.frame_height {
        return Err(HelioFrameError::Config(format!(
            "tile at ({}, {}) with size {}x{} exceeds frame bounds {}x{}",
            tile.x, tile.y, tile.width, tile.height, plan.frame_width, plan.frame_height
        )));
    }

    let expected = tile
        .width
        .checked_mul(tile.height)
        .and_then(|px| px.checked_mul(plan.channels))
        .ok_or_else(|| HelioFrameError::Config("tile pixel count overflow".to_string()))?;

    if tile.pixels.len() != expected {
        return Err(HelioFrameError::Config(format!(
            "tile pixel length mismatch at ({}, {}): expected {}, got {}",
            tile.x,
            tile.y,
            expected,
            tile.pixels.len()
        )));
    }

    Ok(())
}

fn accumulate_tile(
    tile: &FrameTile,
    plan: &StitchPlan,
    accum: &mut [f32],
    weights: &mut [f32],
    coverage: &mut [u16],
) {
    let has_left_overlap = tile.x > 0;
    let has_right_overlap = tile.x + tile.width < plan.frame_width;
    let has_top_overlap = tile.y > 0;
    let has_bottom_overlap = tile.y + tile.height < plan.frame_height;

    for local_y in 0..tile.height {
        for local_x in 0..tile.width {
            let wx = overlap_weight(
                local_x,
                tile.width,
                plan.overlap,
                has_left_overlap,
                has_right_overlap,
            );
            let wy = overlap_weight(
                local_y,
                tile.height,
                plan.overlap,
                has_top_overlap,
                has_bottom_overlap,
            );
            let weight = wx * wy;

            let global_x = tile.x + local_x;
            let global_y = tile.y + local_y;
            let sample_idx = global_y * plan.frame_width + global_x;
            let out_base = sample_idx * plan.channels;
            let tile_base = (local_y * tile.width + local_x) * plan.channels;

            weights[sample_idx] += weight;
            coverage[sample_idx] = coverage[sample_idx].saturating_add(1);

            for c in 0..plan.channels {
                accum[out_base + c] += f32::from(tile.pixels[tile_base + c]) * weight;
            }
        }
    }
}

fn overlap_weight(
    pos: usize,
    length: usize,
    overlap: usize,
    has_start_overlap: bool,
    has_end_overlap: bool,
) -> f32 {
    if overlap == 0 || length <= 1 {
        return 1.0;
    }

    let ramp = overlap.min(length.saturating_sub(1));
    if ramp == 0 {
        return 1.0;
    }

    let mut weight = 1.0f32;

    if has_start_overlap && pos < ramp {
        weight *= (pos as f32 + 1.0) / (ramp as f32 + 1.0);
    }

    if has_end_overlap {
        let distance_to_end = length.saturating_sub(pos + 1);
        if distance_to_end < ramp {
            weight *= (distance_to_end as f32 + 1.0) / (ramp as f32 + 1.0);
        }
    }

    weight.max(1e-5)
}

fn build_seam_debug_overlay(
    output: &[u8],
    coverage: &[u16],
    width: usize,
    height: usize,
    channels: usize,
) -> Vec<u8> {
    let mut overlay = output.to_vec();

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if coverage[idx] <= 1 {
                continue;
            }

            let alpha = ((coverage[idx] - 1) as f32 / 3.0).clamp(0.2, 0.8);
            let base = idx * channels;
            overlay[base] = blend_u8(overlay[base], 255, alpha);
            if channels > 1 {
                overlay[base + 1] = blend_u8(overlay[base + 1], 40, alpha);
            }
            if channels > 2 {
                overlay[base + 2] = blend_u8(overlay[base + 2], 40, alpha);
            }
        }
    }

    overlay
}

fn blend_u8(current: u8, target: u8, alpha: f32) -> u8 {
    let c = f32::from(current);
    let t = f32::from(target);
    (c * (1.0 - alpha) + t * alpha).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::{stitch_tiles, FrameTile, StitchPlan};

    #[test]
    fn weighted_overlap_blending_avoids_hard_seams_on_gradient() {
        let frame_width = 64;
        let frame_height = 40;
        let channels = 3;
        let overlap = 8;

        let base = gradient_frame(frame_width, frame_height, channels);

        let tiles = vec![
            tile_from_frame(&base, frame_width, channels, 0, 0, 40, frame_height, 12),
            tile_from_frame(&base, frame_width, channels, 24, 0, 40, frame_height, -12),
        ];

        let stitched = stitch_tiles(
            &tiles,
            &StitchPlan {
                frame_width,
                frame_height,
                channels,
                overlap,
                seam_debug: false,
            },
        )
        .expect("stitch should succeed");

        let seam_x = 24;
        let max_delta = seam_energy(
            &stitched.pixels,
            frame_width,
            frame_height,
            channels,
            seam_x,
        );
        assert!(
            max_delta < 12.0,
            "seam delta should remain low after blending, got {max_delta}"
        );
    }

    #[test]
    fn seam_debug_overlay_highlights_overlap_regions() {
        let tiles = vec![
            FrameTile {
                x: 0,
                y: 0,
                width: 8,
                height: 4,
                pixels: vec![80; 8 * 4 * 3],
            },
            FrameTile {
                x: 4,
                y: 0,
                width: 8,
                height: 4,
                pixels: vec![160; 8 * 4 * 3],
            },
        ];

        let stitched = stitch_tiles(
            &tiles,
            &StitchPlan {
                frame_width: 12,
                frame_height: 4,
                channels: 3,
                overlap: 4,
                seam_debug: true,
            },
        )
        .expect("stitch should succeed");

        let seam_debug = stitched
            .seam_debug_pixels
            .expect("debug overlay should be present");
        let overlap_pixel = (1 * 12 + 5) * 3;
        assert!(
            seam_debug[overlap_pixel] > stitched.pixels[overlap_pixel],
            "debug overlay should increase red channel in overlap"
        );
    }

    fn gradient_frame(width: usize, height: usize, channels: usize) -> Vec<u8> {
        let mut frame = vec![0u8; width * height * channels];
        for y in 0..height {
            for x in 0..width {
                let base = (y * width + x) * channels;
                let value = ((x as f32 / (width - 1) as f32) * 255.0).round() as u8;
                frame[base] = value;
                frame[base + 1] = (255 - value) / 2;
                frame[base + 2] = 255 - value;
            }
        }
        frame
    }

    fn tile_from_frame(
        frame: &[u8],
        frame_width: usize,
        channels: usize,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        tile_bias: i16,
    ) -> FrameTile {
        let mut pixels = vec![0u8; width * height * channels];
        for local_y in 0..height {
            for local_x in 0..width {
                let src_base = ((y + local_y) * frame_width + (x + local_x)) * channels;
                let dst_base = (local_y * width + local_x) * channels;
                for c in 0..channels {
                    let biased = i16::from(frame[src_base + c]) + tile_bias;
                    pixels[dst_base + c] = biased.clamp(0, 255) as u8;
                }
            }
        }

        FrameTile {
            x,
            y,
            width,
            height,
            pixels,
        }
    }

    fn seam_energy(
        frame: &[u8],
        width: usize,
        height: usize,
        channels: usize,
        seam_x: usize,
    ) -> f32 {
        let mut peak = 0f32;
        for y in 0..height {
            let left = (y * width + seam_x.saturating_sub(1)) * channels;
            let right = (y * width + seam_x.min(width - 1)) * channels;
            let mut delta = 0f32;
            for c in 0..channels {
                delta += (f32::from(frame[left + c]) - f32::from(frame[right + c])).abs();
            }
            peak = peak.max(delta / channels as f32);
        }
        peak
    }
}
