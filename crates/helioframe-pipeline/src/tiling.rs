use helioframe_core::{TemporalWindow, TileCoordinate, WindowTileManifest};

pub fn build_window_tile_manifests(
    windows: &[TemporalWindow],
    frame_width: usize,
    frame_height: usize,
    tile_size: usize,
    overlap: usize,
) -> Vec<WindowTileManifest> {
    let tiles = generate_tiles(frame_width, frame_height, tile_size, overlap);

    windows
        .iter()
        .enumerate()
        .map(|(window_index, window)| WindowTileManifest {
            window_index,
            start_frame: window.start_frame,
            end_frame_exclusive: window.end_frame_exclusive,
            tile_size,
            overlap,
            tiles: tiles.clone(),
        })
        .collect()
}

pub fn generate_tiles(
    frame_width: usize,
    frame_height: usize,
    tile_size: usize,
    overlap: usize,
) -> Vec<TileCoordinate> {
    if frame_width == 0 || frame_height == 0 || tile_size == 0 || tile_size <= overlap {
        return Vec::new();
    }

    let x_positions = tile_positions(frame_width, tile_size, overlap);
    let y_positions = tile_positions(frame_height, tile_size, overlap);
    let mut tiles = Vec::with_capacity(x_positions.len() * y_positions.len());

    for &y in &y_positions {
        for &x in &x_positions {
            let width = tile_size.min(frame_width - x);
            let height = tile_size.min(frame_height - y);
            tiles.push(TileCoordinate {
                x,
                y,
                width,
                height,
            });
        }
    }

    tiles
}

fn tile_positions(full_extent: usize, tile_size: usize, overlap: usize) -> Vec<usize> {
    if full_extent <= tile_size {
        return vec![0];
    }

    let stride = tile_size - overlap;
    let mut positions = Vec::new();
    let mut cursor = 0usize;

    while cursor + tile_size < full_extent {
        positions.push(cursor);
        cursor += stride;
    }

    let final_start = full_extent - tile_size;
    if positions.last().copied() != Some(final_start) {
        positions.push(final_start);
    }

    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use helioframe_core::TemporalWindow;

    #[test]
    fn generate_tiles_covers_full_frame_with_overlap() {
        let tiles = generate_tiles(1024, 768, 512, 64);
        let xs: Vec<usize> = tiles.iter().map(|tile| tile.x).collect();
        let ys: Vec<usize> = tiles.iter().map(|tile| tile.y).collect();

        assert!(xs.contains(&0));
        assert!(xs.contains(&(1024 - 512)));
        assert!(ys.contains(&0));
        assert!(ys.contains(&(768 - 512)));
        assert!(tiles.iter().all(|tile| tile.width <= 512));
        assert!(tiles.iter().all(|tile| tile.height <= 512));
    }

    #[test]
    fn generate_tiles_handles_smaller_than_tile_extent() {
        let tiles = generate_tiles(320, 180, 512, 64);
        assert_eq!(
            tiles,
            vec![TileCoordinate {
                x: 0,
                y: 0,
                width: 320,
                height: 180
            }]
        );
    }

    #[test]
    fn window_tile_manifest_repeats_tile_coordinates_per_window() {
        let windows = vec![
            TemporalWindow {
                start_frame: 0,
                end_frame_exclusive: 8,
                anchor_frames: vec![0, 4],
            },
            TemporalWindow {
                start_frame: 8,
                end_frame_exclusive: 12,
                anchor_frames: vec![8],
            },
        ];

        let manifests = build_window_tile_manifests(&windows, 640, 640, 512, 64);
        assert_eq!(manifests.len(), 2);
        assert_eq!(manifests[0].tiles, manifests[1].tiles);
        assert_eq!(manifests[0].window_index, 0);
        assert_eq!(manifests[1].window_index, 1);
        assert_eq!(manifests[0].tile_size, 512);
        assert_eq!(manifests[0].overlap, 64);
    }
}
