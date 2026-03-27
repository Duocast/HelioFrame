use std::{path::Path, process::Command};

use helioframe_core::{HelioFrameError, HelioFrameResult};
use helioframe_video::DecodedFrames;

use crate::stages::{SceneBoundary, ShotDetectionArtifact};

pub const DEFAULT_SCDET_THRESHOLD: f64 = 12.0;

pub fn detect_shots(
    input: &Path,
    decoded: &DecodedFrames,
    threshold: f64,
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

    Ok(ShotDetectionArtifact {
        threshold,
        frame_count: decoded.frame_count,
        boundaries,
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
