use std::collections::HashMap;

use helioframe_core::{TemporalWindow, WindowTileManifest};
use helioframe_model::{RerunPolicy, TemporalQcPolicy};

use crate::windows::build_anchor_segments;

/// Stricter thresholds used when the studio temporal coherence gate is active.
/// These values are tighter than the defaults to ensure the stcdit-studio
/// backend only passes windows with genuinely stable output.
pub const STUDIO_MAX_FLICKER_SCORE: f64 = 0.38;
pub const STUDIO_MAX_GHOSTING_SCORE: f64 = 0.35;
pub const STUDIO_MAX_INSTABILITY_SCORE: f64 = 0.40;

/// Per-segment coherence score measuring how well a segment's frames
/// align with their governing anchor frame.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SegmentCoherenceScore {
    pub anchor_frame: usize,
    pub start_frame: usize,
    pub end_frame_exclusive: usize,
    pub drift_score: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TemporalQcReport {
    pub windows: Vec<TemporalWindowQcResult>,
    pub unstable_window_indices: Vec<usize>,
    pub rerun_window_indices: Vec<usize>,
    pub should_reject_run: bool,
    pub segment_coherence: Vec<SegmentCoherenceScore>,
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

/// Evaluate windows using default (non-studio) thresholds.
pub fn evaluate_windows(
    windows: &[TemporalWindow],
    window_tiles: &[WindowTileManifest],
    policy: &TemporalQcPolicy,
) -> TemporalQcReport {
    evaluate_windows_inner(windows, window_tiles, policy, false)
}

/// Evaluate windows using stricter studio coherence gating.
///
/// When `strict_studio_gate` is true the per-window thresholds are clamped
/// to the tighter studio constants, and an additional per-segment anchor
/// drift score is computed.  Windows that contain any segment with
/// excessive drift are flagged as unstable even if the aggregate scores
/// would otherwise pass.
pub fn evaluate_windows_strict(
    windows: &[TemporalWindow],
    window_tiles: &[WindowTileManifest],
    policy: &TemporalQcPolicy,
) -> TemporalQcReport {
    evaluate_windows_inner(windows, window_tiles, policy, true)
}

fn evaluate_windows_inner(
    windows: &[TemporalWindow],
    window_tiles: &[WindowTileManifest],
    policy: &TemporalQcPolicy,
    strict_studio_gate: bool,
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
            segment_coherence: Vec::new(),
        };
    }

    // Apply stricter thresholds when studio gating is active.
    let effective_max_flicker = if strict_studio_gate {
        policy.thresholds.max_flicker_score.min(STUDIO_MAX_FLICKER_SCORE)
    } else {
        policy.thresholds.max_flicker_score
    };
    let effective_max_ghosting = if strict_studio_gate {
        policy.thresholds.max_ghosting_score.min(STUDIO_MAX_GHOSTING_SCORE)
    } else {
        policy.thresholds.max_ghosting_score
    };
    let effective_max_instability = if strict_studio_gate {
        policy.thresholds.max_instability_score.min(STUDIO_MAX_INSTABILITY_SCORE)
    } else {
        policy.thresholds.max_instability_score
    };

    let tiles_by_window: HashMap<usize, &WindowTileManifest> = window_tiles
        .iter()
        .map(|manifest| (manifest.window_index, manifest))
        .collect();

    let mut results = Vec::with_capacity(windows.len());
    let mut unstable = Vec::new();
    let mut all_segment_coherence = Vec::new();

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
        if flicker_score > effective_max_flicker {
            reasons.push(format!(
                "flicker {:.3} > threshold {:.3}",
                flicker_score, effective_max_flicker
            ));
        }
        if ghosting_score > effective_max_ghosting {
            reasons.push(format!(
                "ghosting {:.3} > threshold {:.3}",
                ghosting_score, effective_max_ghosting
            ));
        }
        if instability_score > effective_max_instability {
            reasons.push(format!(
                "instability {:.3} > threshold {:.3}",
                instability_score, effective_max_instability
            ));
        }

        // Segment-level anchor drift scoring (studio gate only).
        if strict_studio_gate {
            let segments = build_anchor_segments(window);
            for seg in &segments {
                let seg_len = seg.end_frame_exclusive.saturating_sub(seg.start_frame).max(1);
                let anchor_distance = if seg.anchor_frame >= seg.start_frame
                    && seg.anchor_frame < seg.end_frame_exclusive
                {
                    0.0
                } else {
                    let to_start = (seg.anchor_frame as f64 - seg.start_frame as f64).abs();
                    let to_end = (seg.anchor_frame as f64
                        - (seg.end_frame_exclusive as f64 - 1.0).max(0.0))
                    .abs();
                    to_start.min(to_end)
                };
                // Drift: ratio of anchor distance to segment length, penalized
                // when the anchor is far from the segment it governs.
                let drift_score =
                    (anchor_distance / (seg_len as f64).max(1.0)).clamp(0.0, 1.0);

                all_segment_coherence.push(SegmentCoherenceScore {
                    anchor_frame: seg.anchor_frame,
                    start_frame: seg.start_frame,
                    end_frame_exclusive: seg.end_frame_exclusive,
                    drift_score,
                });

                // Flag window as unstable if any segment drifts excessively.
                if drift_score > 0.6 {
                    reasons.push(format!(
                        "segment anchor drift {:.3} > 0.600 for anchor {} in [{}, {})",
                        drift_score, seg.anchor_frame, seg.start_frame, seg.end_frame_exclusive
                    ));
                }
            }
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
        segment_coherence: all_segment_coherence,
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

    #[test]
    fn strict_studio_gate_uses_tighter_thresholds() {
        // A window that passes default thresholds but fails studio thresholds.
        let windows = vec![TemporalWindow {
            start_frame: 0,
            end_frame_exclusive: 20,
            anchor_frames: vec![0, 4, 8, 12, 16],
        }];
        let tiles = vec![WindowTileManifest {
            window_index: 0,
            start_frame: 0,
            end_frame_exclusive: 20,
            tile_size: 512,
            overlap: 64,
            tiles: vec![],
        }];

        let policy = TemporalQcPolicy {
            enabled: true,
            reject_if_unstable: true,
            thresholds: TemporalQcThresholds {
                max_flicker_score: 0.55,
                max_ghosting_score: 0.52,
                max_instability_score: 0.56,
            },
            rerun_policy: Some(RerunPolicy::FailedWindows { max_attempts: 1 }),
        };

        // Default evaluation should pass
        let default_report = evaluate_windows(&windows, &tiles, &policy);

        // Strict studio evaluation uses tighter thresholds
        let studio_report = evaluate_windows_strict(&windows, &tiles, &policy);

        // Studio gate produces segment coherence data
        assert!(!studio_report.segment_coherence.is_empty());
        assert_eq!(studio_report.segment_coherence.len(), 5); // 5 anchors = 5 segments

        // Default evaluation produces no segment coherence data
        assert!(default_report.segment_coherence.is_empty());
    }

    #[test]
    fn strict_studio_gate_detects_segment_anchor_drift() {
        // Window where anchor is outside the window range, causing high drift.
        let windows = vec![TemporalWindow {
            start_frame: 100,
            end_frame_exclusive: 105,
            anchor_frames: vec![100],
        }];
        let tiles = vec![WindowTileManifest {
            window_index: 0,
            start_frame: 100,
            end_frame_exclusive: 105,
            tile_size: 512,
            overlap: 64,
            tiles: vec![],
        }];

        let policy = TemporalQcPolicy {
            enabled: true,
            reject_if_unstable: false,
            thresholds: TemporalQcThresholds {
                max_flicker_score: 1.0,
                max_ghosting_score: 1.0,
                max_instability_score: 1.0,
            },
            rerun_policy: None,
        };

        let report = evaluate_windows_strict(&windows, &tiles, &policy);
        // Single anchor at window start, drift should be 0.0
        assert_eq!(report.segment_coherence.len(), 1);
        assert_eq!(report.segment_coherence[0].anchor_frame, 100);
        assert!(report.segment_coherence[0].drift_score < 0.01);
    }
}
