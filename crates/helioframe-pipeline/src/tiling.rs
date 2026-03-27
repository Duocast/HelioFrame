use helioframe_core::{Resolution, SpatialTile, TemporalWindow, WindowTileManifest};

pub fn build_window_tile_manifest(
    windows: &[TemporalWindow],
    resolution: Resolution,
    tile_size: usize,
    overlap: usize,
) -> Vec<WindowTileManifest> {
    let tiles = build_spatial_tiles(resolution, tile_size, overlap);
    windows
        .iter()
        .enumerate()
        .map(|(window_index, window)| WindowTileManifest {
            window_index,
            start_frame: window.start_frame,
            end_frame_exclusive: window.end_frame_exclusive,
            tiles: tiles.clone(),
        })
        .collect()
}

pub fn build_spatial_tiles(
    resolution: Resolution,
    tile_size: usize,
    overlap: usize,
) -> Vec<SpatialTile> {
    let tile_size = tile_size.max(1) as u32;
    let overlap = overlap.min((tile_size.saturating_sub(1)) as usize) as u32;
    let x_positions = axis_positions(resolution.width, tile_size, overlap);
    let y_positions = axis_positions(resolution.height, tile_size, overlap);

    let mut tile_index = 0usize;
    let mut tiles = Vec::with_capacity(x_positions.len() * y_positions.len());
    for y in y_positions {
        for x in &x_positions {
            let width = tile_size.min(resolution.width.saturating_sub(*x));
            let height = tile_size.min(resolution.height.saturating_sub(y));
            tiles.push(SpatialTile {
                tile_index,
                x: *x,
                y,
                width,
                height,
            });
            tile_index += 1;
        }
    }

    tiles
}

fn axis_positions(extent: u32, tile_size: u32, overlap: u32) -> Vec<u32> {
    if extent == 0 {
        return vec![];
    }
    if extent <= tile_size {
        return vec![0];
    }

    let stride = tile_size.saturating_sub(overlap).max(1);
    let mut positions = vec![0u32];
    loop {
        let current = *positions.last().unwrap_or(&0);
        if current + tile_size >= extent {
            break;
        }
        let next = current + stride;
        let edge_aligned = extent - tile_size;
        positions.push(next.min(edge_aligned));
        if *positions.last().unwrap_or(&0) == edge_aligned {
            break;
        }
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_aware_tiling_covers_full_frame_edges() {
        let tiles = build_spatial_tiles(
            Resolution {
                width: 1920,
                height: 1080,
            },
            512,
            64,
        );

        assert!(!tiles.is_empty());
        let right_edge = tiles
            .iter()
            .map(|tile| tile.x + tile.width)
            .max()
            .expect("tiles should contain a right edge");
        let bottom_edge = tiles
            .iter()
            .map(|tile| tile.y + tile.height)
            .max()
            .expect("tiles should contain a bottom edge");
        assert_eq!(right_edge, 1920);
        assert_eq!(bottom_edge, 1080);
    }

    #[test]
    fn tile_manifest_repeats_exact_coords_per_window() {
        let windows = vec![
            TemporalWindow {
                start_frame: 0,
                end_frame_exclusive: 8,
                anchor_frames: vec![0, 4],
            },
            TemporalWindow {
                start_frame: 8,
                end_frame_exclusive: 16,
                anchor_frames: vec![8, 12],
            },
        ];

        let manifest = build_window_tile_manifest(
            &windows,
            Resolution {
                width: 3840,
                height: 2160,
            },
            512,
            64,
        );

        assert_eq!(manifest.len(), 2);
        assert_eq!(manifest[0].start_frame, 0);
        assert_eq!(manifest[1].start_frame, 8);
        assert_eq!(manifest[0].tiles, manifest[1].tiles);
    }
}
