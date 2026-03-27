use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use helioframe_core::{HelioFrameError, HelioFrameResult, Resolution};

use crate::DecodedFrames;

#[derive(Debug, Clone)]
pub struct EncodePlan {
    pub output_resolution: Resolution,
    pub preserve_audio: bool,
    pub container_hint: &'static str,
}

#[derive(Debug, Clone)]
pub struct EncodeResult {
    pub output_path: PathBuf,
}

pub fn encode_from_frame_directory(
    decoded: &DecodedFrames,
    output_path: &Path,
    plan: &EncodePlan,
) -> HelioFrameResult<EncodeResult> {
    let concat_path = decoded.frames_dir.join("frames.ffconcat");
    let frame_paths = gather_frame_paths(decoded)?;

    write_ffconcat_file(&concat_path, &frame_paths, &decoded.timestamps_path)?;

    if plan.preserve_audio {
        if let Some(audio_path) = decoded.audio_path.as_ref() {
            let output = run_encode(Some(audio_path), &concat_path, output_path, plan, true)?;
            if output.status.success() {
                return Ok(EncodeResult {
                    output_path: output_path.to_path_buf(),
                });
            }

            let fallback = run_encode(Some(audio_path), &concat_path, output_path, plan, false)?;
            if fallback.status.success() {
                return Ok(EncodeResult {
                    output_path: output_path.to_path_buf(),
                });
            }

            return Err(HelioFrameError::Config(format!(
                "ffmpeg encode failed with copied and transcoded audio: {}; {}",
                String::from_utf8_lossy(&output.stderr).trim(),
                String::from_utf8_lossy(&fallback.stderr).trim()
            )));
        }
    }

    let no_audio = run_encode(None, &concat_path, output_path, plan, false)?;
    if no_audio.status.success() {
        return Ok(EncodeResult {
            output_path: output_path.to_path_buf(),
        });
    }

    Err(HelioFrameError::Config(format!(
        "ffmpeg encode failed: {}",
        String::from_utf8_lossy(&no_audio.stderr).trim()
    )))
}

fn gather_frame_paths(decoded: &DecodedFrames) -> HelioFrameResult<Vec<PathBuf>> {
    let mut frames = Vec::with_capacity(decoded.frame_count);
    for index in 0..decoded.frame_count {
        let frame = decoded.frames_dir.join(format!("frame_{index:010}.png"));
        if !frame.exists() {
            return Err(HelioFrameError::Config(format!(
                "missing frame {}",
                frame.display()
            )));
        }
        frames.push(frame);
    }
    Ok(frames)
}

fn write_ffconcat_file(
    concat_path: &Path,
    frame_paths: &[PathBuf],
    timestamps_path: &Path,
) -> HelioFrameResult<()> {
    let raw_timestamps = fs::read_to_string(timestamps_path).map_err(|err| {
        HelioFrameError::Config(format!(
            "failed to read frame timestamp file {}: {err}",
            timestamps_path.display()
        ))
    })?;

    let timestamps = raw_timestamps
        .lines()
        .filter_map(|line| line.trim().parse::<f64>().ok())
        .collect::<Vec<_>>();

    if timestamps.len() != frame_paths.len() {
        return Err(HelioFrameError::Config(format!(
            "timestamp/frame count mismatch: {} timestamps vs {} frames",
            timestamps.len(),
            frame_paths.len()
        )));
    }

    if frame_paths.is_empty() {
        return Err(HelioFrameError::Config(
            "no frames found for encode".to_string(),
        ));
    }

    let mut ffconcat = String::from("ffconcat version 1.0\n");

    for index in 0..frame_paths.len() {
        let path = frame_paths[index]
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "'\\''");
        ffconcat.push_str(&format!("file '{path}'\n"));

        if index + 1 < timestamps.len() {
            let delta = (timestamps[index + 1] - timestamps[index]).max(1.0 / 240.0);
            ffconcat.push_str(&format!("duration {delta:.9}\n"));
        }
    }

    let last_path = frame_paths
        .last()
        .expect("frame_paths should be non-empty")
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('\'', "'\\''");
    ffconcat.push_str(&format!("file '{last_path}'\n"));

    fs::write(concat_path, ffconcat).map_err(|err| {
        HelioFrameError::Config(format!(
            "failed to write concat file {}: {err}",
            concat_path.display()
        ))
    })
}

fn run_encode(
    audio_path: Option<&Path>,
    concat_path: &Path,
    output_path: &Path,
    plan: &EncodePlan,
    copy_audio: bool,
) -> HelioFrameResult<Output> {
    let scale_filter = format!(
        "scale={}:{}:flags=lanczos",
        plan.output_resolution.width, plan.output_resolution.height
    );

    let mut command = Command::new("ffmpeg");
    command
        .arg("-y")
        .arg("-v")
        .arg("error")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(concat_path);

    if let Some(audio) = audio_path {
        command.arg("-i").arg(audio);
        command.arg("-map").arg("0:v:0");
        command.arg("-map").arg("1:a:0");
    } else {
        command.arg("-map").arg("0:v:0");
    }

    command
        .arg("-vf")
        .arg(scale_filter)
        .arg("-c:v")
        .arg("libx264")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-fps_mode:v")
        .arg("passthrough")
        .arg("-movflags")
        .arg("+faststart");

    if audio_path.is_some() {
        if copy_audio {
            command.arg("-c:a").arg("copy");
        } else {
            command.arg("-c:a").arg("aac").arg("-b:a").arg("192k");
        }
    }

    command.arg(output_path);

    command
        .output()
        .map_err(|err| HelioFrameError::Config(format!("failed to execute ffmpeg encode: {err}")))
}
