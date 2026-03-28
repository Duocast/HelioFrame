use std::collections::HashMap;

use helioframe_core::{TemporalWindow, WindowTileManifest};
use helioframe_model::{RerunPolicy, TemporalQcPolicy};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TemporalQcReport {
    pub windows: Vec<TemporalWindowQcResult>,
    pub unstable_window_indices: Vec<usize>,
    pub rerun_window_indices: Vec<usize>,
    pub should_reject_run: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TemporalWindowQcResult {
    pub window_index: usize,
    pub start_frame: usize,
    pub end_frame_exclusive: usize,
    pub flicker_score: f64,
    pub ghosting_score: f64,
    pub instability_score: f64,
    pub unstable: bool,
    pub reasons: Vec<String>,
}

pub fn evaluate_windows(
    windows: &[TemporalWindow],
    window_tiles: &[WindowTileManifest],
    policy: &TemporalQcPolicy,
) -> TemporalQcReport {
    if !policy.enabled {
        let passthrough = windows
            .iter()
            .enumerate()
            .map(|(window_index, window)| TemporalWindowQcResult {
                window_index,
                start_frame: window.start_frame,
                end_frame_exclusive: window.end_frame_exclusive,
                flicker_score: 0.0,
                ghosting_score: 0.0,
                instability_score: 0.0,
                unstable: false,
                reasons: vec!["temporal QC disabled".to_string()],
            })
            .collect::<Vec<_>>();

        return TemporalQcReport {
            windows: passthrough,
            unstable_window_indices: Vec::new(),
            rerun_window_indices: Vec::new(),
            should_reject_run: false,
        };
    }

    let tiles_by_window: HashMap<usize, &WindowTileManifest> = window_tiles
        .iter()
        .map(|manifest| (manifest.window_index, manifest))
        .collect();

    let mut results = Vec::with_capacity(windows.len());
    let mut unstable = Vec::new();

    for (window_index, window) in windows.iter().enumerate() {
        let tile_manifest = tiles_by_window.get(&window_index).copied();
        let window_len = window
            .end_frame_exclusive
            .saturating_sub(window.start_frame)
            .max(1);

        let anchor_gap = if window.anchor_frames.len() <= 1 {
            window_len as f64
        } else {
            average_anchor_gap(&window.anchor_frames).max(1.0)
        };
        let normalized_window_len = (window_len as f64 / 24.0).clamp(0.0, 1.5);

        // Proxy-only metrics until frame-difference metrics are wired in.
        let flicker_score = ((normalized_window_len - 0.35).max(0.0) * 0.45
            + (anchor_gap / 10.0).clamp(0.0, 1.0) * 0.55)
            .clamp(0.0, 1.0);

        let overlap_ratio = tile_manifest
            .map(|manifest| {
                if manifest.tile_size == 0 {
                    0.0
                } else {
                    manifest.overlap as f64 / manifest.tile_size as f64
                }
            })
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let ghosting_score =
            ((1.0 - overlap_ratio) * 0.15 + normalized_window_len * 0.2).clamp(0.0, 1.0);

        let instability_score = (flicker_score * 0.55 + ghosting_score * 0.45).clamp(0.0, 1.0);

        let mut reasons = Vec::new();
        if flicker_score > policy.thresholds.max_flicker_score {
            reasons.push(format!(
                "flicker {:.3} > threshold {:.3}",
                flicker_score, policy.thresholds.max_flicker_score
            ));
        }
        if ghosting_score > policy.thresholds.max_ghosting_score {
            reasons.push(format!(
                "ghosting {:.3} > threshold {:.3}",
                ghosting_score, policy.thresholds.max_ghosting_score
            ));
        }
        if instability_score > policy.thresholds.max_instability_score {
            reasons.push(format!(
                "instability {:.3} > threshold {:.3}",
                instability_score, policy.thresholds.max_instability_score
            ));
        }

        let is_unstable = !reasons.is_empty();
        if is_unstable {
            unstable.push(window_index);
        }

        results.push(TemporalWindowQcResult {
            window_index,
            start_frame: window.start_frame,
            end_frame_exclusive: window.end_frame_exclusive,
            flicker_score,
            ghosting_score,
            instability_score,
            unstable: is_unstable,
            reasons,
        });
    }

    let rerun_window_indices = select_rerun_windows(&unstable, policy.rerun_policy.as_ref());

    TemporalQcReport {
        windows: results,
        unstable_window_indices: unstable.clone(),
        rerun_window_indices,
        should_reject_run: policy.reject_if_unstable && !unstable.is_empty(),
    }
}

pub fn select_rerun_windows(
    unstable_window_indices: &[usize],
    rerun_policy: Option<&RerunPolicy>,
) -> Vec<usize> {
    match rerun_policy {
        Some(RerunPolicy::Disabled) | None => Vec::new(),
        Some(RerunPolicy::FailedWindows { max_attempts }) if *max_attempts > 0 => {
            unstable_window_indices.to_vec()
        }
        Some(RerunPolicy::FailedWindows { .. }) => Vec::new(),
    }
}

fn average_anchor_gap(anchor_frames: &[usize]) -> f64 {
    if anchor_frames.len() <= 1 {
        return 1.0;
    }

    let total_gap: usize = anchor_frames
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0]))
        .sum();
    total_gap as f64 / (anchor_frames.len() as f64 - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use helioframe_model::{RerunPolicy, TemporalQcThresholds};

    #[test]
    fn qc_marks_unstable_windows_and_schedules_reruns_only_for_failures() {
        let windows = vec![
            TemporalWindow {
                start_frame: 0,
                end_frame_exclusive: 8,
                anchor_frames: vec![0, 4],
            },
            TemporalWindow {
                start_frame: 8,
                end_frame_exclusive: 40,
                anchor_frames: vec![8],
            },
        ];
        let tiles = vec![
            WindowTileManifest {
                window_index: 0,
                start_frame: 0,
                end_frame_exclusive: 8,
                tile_size: 1024,
                overlap: 192,
                tiles: vec![],
            },
            WindowTileManifest {
                window_index: 1,
                start_frame: 8,
                end_frame_exclusive: 40,
                tile_size: 1024,
                overlap: 0,
                tiles: vec![],
            },
        ];

        let report = evaluate_windows(
            &windows,
            &tiles,
            &TemporalQcPolicy {
                enabled: true,
                reject_if_unstable: true,
                thresholds: TemporalQcThresholds {
                    max_flicker_score: 0.45,
                    max_ghosting_score: 0.45,
                    max_instability_score: 0.45,
                },
                rerun_policy: Some(RerunPolicy::FailedWindows { max_attempts: 1 }),
            },
        );

        assert_eq!(report.unstable_window_indices, vec![1]);
        assert_eq!(report.rerun_window_indices, vec![1]);
        assert!(report.should_reject_run);
    }

    #[test]
    fn qc_can_disable_rerun_policy_without_disabling_window_scoring() {
        let windows = vec![TemporalWindow {
            start_frame: 0,
            end_frame_exclusive: 48,
            anchor_frames: vec![0],
        }];
        let tiles = vec![WindowTileManifest {
            window_index: 0,
            start_frame: 0,
            end_frame_exclusive: 48,
            tile_size: 1024,
            overlap: 0,
            tiles: vec![],
        }];

        let report = evaluate_windows(
            &windows,
            &tiles,
            &TemporalQcPolicy {
                enabled: true,
                reject_if_unstable: false,
                thresholds: TemporalQcThresholds {
                    max_flicker_score: 0.2,
                    max_ghosting_score: 0.2,
                    max_instability_score: 0.2,
                },
                rerun_policy: None,
            },
        );

        assert_eq!(report.unstable_window_indices, vec![0]);
        assert!(report.rerun_window_indices.is_empty());
        assert!(!report.should_reject_run);
    }
}
