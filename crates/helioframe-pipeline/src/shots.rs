use std::{path::Path, process::Command};

use helioframe_core::{HelioFrameError, HelioFrameResult};
use helioframe_video::DecodedFrames;

use crate::stages::{SceneBoundary, ShotDetectionArtifact, TemporalWindow};

pub const DEFAULT_SCDET_THRESHOLD: f64 = 12.0;

pub fn detect_shots(
    input: &Path,
    decoded: &DecodedFrames,
    threshold: f64,
    max_window_size: usize,
) -> HelioFrameResult<ShotDetectionArtifact> {
    let timestamps = read_timestamps(&decoded.timestamps_path)?;
    if timestamps.len() != decoded.frame_count {
        return Err(HelioFrameError::Config(format!(
            "decoded frame count mismatch (timestamps={}, frame_count={})",
            timestamps.len(),
            decoded.frame_count
        )));
    }

    let mut boundaries = detect_boundaries_with_ffmpeg(input, &timestamps, threshold)?;
    boundaries.sort_by_key(|boundary| boundary.frame_index);
    boundaries.dedup_by_key(|boundary| boundary.frame_index);

    let windows = split_into_windows(decoded.frame_count, &boundaries, max_window_size);

    Ok(ShotDetectionArtifact {
        threshold,
        frame_count: decoded.frame_count,
        boundaries,
        windows,
    })
}

fn detect_boundaries_with_ffmpeg(
    input: &Path,
    timestamps: &[f64],
    threshold: f64,
) -> HelioFrameResult<Vec<SceneBoundary>> {
    let filter = format!("scdet=threshold={threshold},metadata=print:file=-");
    let output = Command::new("ffmpeg")
        .arg("-v")
        .arg("error")
        .arg("-i")
        .arg(input)
        .arg("-an")
        .arg("-vf")
        .arg(filter)
        .arg("-f")
        .arg("null")
        .arg("-")
        .output()
        .map_err(|err| HelioFrameError::Config(format!("failed to execute ffmpeg scdet: {err}")))?;

    if !output.status.success() {
        return Err(HelioFrameError::Config(format!(
            "ffmpeg scdet failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    parse_scdet_metadata(&combined, timestamps)
}

fn parse_scdet_metadata(log: &str, timestamps: &[f64]) -> HelioFrameResult<Vec<SceneBoundary>> {
    let mut boundaries = Vec::new();
    let mut pending_time = None;

    for line in log.lines() {
        if let Some((_, value)) = line.split_once("lavfi.scd.time=") {
            let timestamp_seconds = value.trim().parse::<f64>().map_err(|err| {
                HelioFrameError::Config(format!(
                    "invalid scdet timestamp '{}': {err}",
                    value.trim()
                ))
            })?;
            pending_time = Some(timestamp_seconds);
            continue;
        }

        if let Some((_, value)) = line.split_once("lavfi.scd.score=") {
            let Some(timestamp_seconds) = pending_time.take() else {
                continue;
            };
            let score = value.trim().parse::<f64>().map_err(|err| {
                HelioFrameError::Config(format!("invalid scdet score '{}': {err}", value.trim()))
            })?;

            let frame_index = frame_index_for_timestamp(timestamps, timestamp_seconds)
                .unwrap_or_else(|| timestamps.len().saturating_sub(1));
            if frame_index > 0 {
                boundaries.push(SceneBoundary {
                    frame_index,
                    timestamp_seconds,
                    score,
                });
            }
        }
    }

    Ok(boundaries)
}

fn split_into_windows(
    frame_count: usize,
    boundaries: &[SceneBoundary],
    max_window_size: usize,
) -> Vec<TemporalWindow> {
    if frame_count == 0 {
        return vec![];
    }

    let mut windows = Vec::new();
    let max_window_size = max_window_size.max(1);
    let mut current_start = 0usize;

    let scene_starts = boundaries
        .iter()
        .map(|boundary| boundary.frame_index)
        .filter(|index| *index > 0 && *index < frame_count);

    for boundary_start in scene_starts.chain(std::iter::once(frame_count)) {
        let mut chunk_start = current_start;
        while chunk_start < boundary_start {
            let chunk_end = (chunk_start + max_window_size).min(boundary_start);
            windows.push(TemporalWindow {
                start_frame: chunk_start,
                end_frame_exclusive: chunk_end,
            });
            chunk_start = chunk_end;
        }
        current_start = boundary_start;
    }

    windows
}

fn frame_index_for_timestamp(timestamps: &[f64], timestamp: f64) -> Option<usize> {
    if timestamps.is_empty() {
        return None;
    }

    let idx = timestamps.partition_point(|frame_ts| *frame_ts + 1e-6 < timestamp);
    Some(idx.min(timestamps.len().saturating_sub(1)))
}

fn read_timestamps(path: &Path) -> HelioFrameResult<Vec<f64>> {
    let contents = std::fs::read_to_string(path).map_err(|err| {
        HelioFrameError::Config(format!(
            "failed to read timestamp file {}: {err}",
            path.display()
        ))
    })?;

    let mut timestamps = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        timestamps.push(line.parse::<f64>().map_err(|err| {
            HelioFrameError::Config(format!(
                "invalid timestamp '{line}' in {}: {err}",
                path.display()
            ))
        })?);
    }

    Ok(timestamps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_split_respect_scene_boundaries_and_max_size() {
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

        let windows = split_into_windows(30, &boundaries, 8);
        let ranges: Vec<(usize, usize)> = windows
            .into_iter()
            .map(|window| (window.start_frame, window.end_frame_exclusive))
            .collect();

        assert_eq!(ranges, vec![(0, 8), (8, 10), (10, 18), (18, 23), (23, 30)]);
    }
}
