use helioframe_core::{SceneBoundary, TemporalWindow, WindowedClipBatch};

/// A segment within a temporal window, associated with its governing anchor frame.
/// Each segment contains frames closest to one particular anchor, enabling
/// anchor-frame-aware restoration where the model conditions on the anchor.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AnchorSegment {
    pub start_frame: usize,
    pub end_frame_exclusive: usize,
    pub anchor_frame: usize,
}

/// Split a temporal window into anchor-governed segments.
///
/// Each frame is assigned to the segment of its nearest anchor frame.
/// Segment boundaries fall at midpoints between consecutive anchors.
pub fn build_anchor_segments(window: &TemporalWindow) -> Vec<AnchorSegment> {
    let start = window.start_frame;
    let end = window.end_frame_exclusive;

    if start >= end {
        return vec![];
    }

    let anchors = &window.anchor_frames;
    if anchors.is_empty() {
        return vec![AnchorSegment {
            start_frame: start,
            end_frame_exclusive: end,
            anchor_frame: start,
        }];
    }

    if anchors.len() == 1 {
        return vec![AnchorSegment {
            start_frame: start,
            end_frame_exclusive: end,
            anchor_frame: anchors[0],
        }];
    }

    let mut segments = Vec::with_capacity(anchors.len());
    for i in 0..anchors.len() {
        let seg_start = if i == 0 {
            start
        } else {
            (anchors[i - 1] + anchors[i]) / 2
        };
        let seg_end = if i == anchors.len() - 1 {
            end
        } else {
            (anchors[i] + anchors[i + 1]) / 2
        };

        if seg_start < seg_end {
            segments.push(AnchorSegment {
                start_frame: seg_start,
                end_frame_exclusive: seg_end,
                anchor_frame: anchors[i],
            });
        }
    }

    segments
}

pub fn build_windows_and_batches(
    frame_count: usize,
    boundaries: &[SceneBoundary],
    max_window_size: usize,
    anchor_frame_stride: usize,
) -> (Vec<TemporalWindow>, Vec<WindowedClipBatch>) {
    let windows = split_into_windows(
        frame_count,
        boundaries,
        max_window_size,
        anchor_frame_stride,
    );
    let batches = windows
        .iter()
        .enumerate()
        .map(|(window_index, window)| WindowedClipBatch {
            window_index,
            start_frame: window.start_frame,
            end_frame_exclusive: window.end_frame_exclusive,
            anchor_frames: window.anchor_frames.clone(),
        })
        .collect();

    (windows, batches)
}

fn split_into_windows(
    frame_count: usize,
    boundaries: &[SceneBoundary],
    max_window_size: usize,
    anchor_frame_stride: usize,
) -> Vec<TemporalWindow> {
    if frame_count == 0 {
        return vec![];
    }

    let max_window_size = max_window_size.max(1);
    let mut windows = Vec::new();

    let mut scene_boundaries = boundaries
        .iter()
        .map(|boundary| boundary.frame_index)
        .filter(|index| *index > 0 && *index < frame_count)
        .collect::<Vec<_>>();
    scene_boundaries.sort_unstable();
    scene_boundaries.dedup();

    let mut current_scene_start = 0usize;

    for scene_end in scene_boundaries
        .into_iter()
        .chain(std::iter::once(frame_count))
    {
        let mut chunk_start = current_scene_start;
        while chunk_start < scene_end {
            let chunk_end = (chunk_start + max_window_size).min(scene_end);
            let anchor_frames = select_anchor_frames(
                chunk_start,
                chunk_end,
                current_scene_start,
                anchor_frame_stride,
            );
            windows.push(TemporalWindow {
                start_frame: chunk_start,
                end_frame_exclusive: chunk_end,
                anchor_frames,
            });
            chunk_start = chunk_end;
        }

        current_scene_start = scene_end;
    }

    windows
}

fn select_anchor_frames(
    window_start: usize,
    window_end_exclusive: usize,
    scene_start: usize,
    anchor_frame_stride: usize,
) -> Vec<usize> {
    if window_start >= window_end_exclusive {
        return Vec::new();
    }

    if anchor_frame_stride == 0 {
        return vec![window_start];
    }

    let mut anchors = Vec::new();
    let relative_offset = (window_start - scene_start) % anchor_frame_stride;
    let first = if relative_offset == 0 {
        window_start
    } else {
        window_start + (anchor_frame_stride - relative_offset)
    };

    let mut frame = first;
    while frame < window_end_exclusive {
        anchors.push(frame);
        frame += anchor_frame_stride;
    }

    if anchors.is_empty() {
        anchors.push(window_start);
    }

    anchors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_never_cross_boundaries() {
        let boundaries = vec![
            SceneBoundary {
                frame_index: 10,
                timestamp_seconds: 0.4,
                score: 18.2,
            },
            SceneBoundary {
                frame_index: 23,
                timestamp_seconds: 0.95,
                score: 22.1,
            },
        ];

        let (windows, _) = build_windows_and_batches(30, &boundaries, 8, 4);
        let ranges: Vec<(usize, usize)> = windows
            .iter()
            .map(|window| (window.start_frame, window.end_frame_exclusive))
            .collect();

        assert_eq!(ranges, vec![(0, 8), (8, 10), (10, 18), (18, 23), (23, 30)]);
    }

    #[test]
    fn anchor_selection_is_deterministic_and_resets_per_scene() {
        let boundaries = vec![SceneBoundary {
            frame_index: 10,
            timestamp_seconds: 0.4,
            score: 18.2,
        }];

        let (windows, batches) = build_windows_and_batches(18, &boundaries, 5, 4);
        assert_eq!(windows[0].anchor_frames, vec![0, 4]);
        assert_eq!(windows[1].anchor_frames, vec![8]);
        assert_eq!(windows[2].anchor_frames, vec![10, 14]);
        assert_eq!(windows[3].anchor_frames, vec![15]);
        assert_eq!(batches[2].anchor_frames, vec![10, 14]);
    }

    #[test]
    fn anchor_segments_split_at_midpoints() {
        let window = TemporalWindow {
            start_frame: 0,
            end_frame_exclusive: 20,
            anchor_frames: vec![0, 4, 8, 12, 16],
        };
        let segments = build_anchor_segments(&window);
        assert_eq!(
            segments,
            vec![
                AnchorSegment { start_frame: 0, end_frame_exclusive: 2, anchor_frame: 0 },
                AnchorSegment { start_frame: 2, end_frame_exclusive: 6, anchor_frame: 4 },
                AnchorSegment { start_frame: 6, end_frame_exclusive: 10, anchor_frame: 8 },
                AnchorSegment { start_frame: 10, end_frame_exclusive: 14, anchor_frame: 12 },
                AnchorSegment { start_frame: 14, end_frame_exclusive: 20, anchor_frame: 16 },
            ]
        );
    }

    #[test]
    fn anchor_segments_single_anchor_covers_full_window() {
        let window = TemporalWindow {
            start_frame: 5,
            end_frame_exclusive: 15,
            anchor_frames: vec![8],
        };
        let segments = build_anchor_segments(&window);
        assert_eq!(
            segments,
            vec![AnchorSegment { start_frame: 5, end_frame_exclusive: 15, anchor_frame: 8 }]
        );
    }

    #[test]
    fn anchor_segments_empty_anchors_fall_back_to_window_start() {
        let window = TemporalWindow {
            start_frame: 10,
            end_frame_exclusive: 20,
            anchor_frames: vec![],
        };
        let segments = build_anchor_segments(&window);
        assert_eq!(
            segments,
            vec![AnchorSegment { start_frame: 10, end_frame_exclusive: 20, anchor_frame: 10 }]
        );
    }

    #[test]
    fn stride_zero_picks_window_start_anchor() {
        let (windows, _) = build_windows_and_batches(9, &[], 4, 0);
        let anchors: Vec<Vec<usize>> = windows
            .into_iter()
            .map(|window| window.anchor_frames)
            .collect();
        assert_eq!(anchors, vec![vec![0], vec![4], vec![8]]);
    }
}
