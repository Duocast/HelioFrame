use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use helioframe_core::{HelioFrameError, HelioFrameResult};

use crate::probe_input;

#[derive(Debug, Clone)]
pub struct DecodePlan {
    pub use_ffmpeg: bool,
    pub preserve_audio: bool,
}

impl Default for DecodePlan {
    fn default() -> Self {
        Self {
            use_ffmpeg: true,
            preserve_audio: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedFrames {
    pub frames_dir: PathBuf,
    pub frame_pattern: String,
    pub timestamps_path: PathBuf,
    pub frame_count: usize,
    pub fps: f64,
    pub duration_seconds: f64,
    pub audio_path: Option<PathBuf>,
}

pub fn decode_to_frame_directory(
    input: &Path,
    frames_dir: &Path,
    plan: &DecodePlan,
) -> HelioFrameResult<DecodedFrames> {
    if !plan.use_ffmpeg {
        return Err(HelioFrameError::Config(
            "decode without ffmpeg is not implemented".to_string(),
        ));
    }

    fs::create_dir_all(frames_dir).map_err(|err| {
        HelioFrameError::Config(format!(
            "failed to create frame directory {}: {err}",
            frames_dir.display()
        ))
    })?;

    let probe = probe_input(input)?;
    let timestamps = ffprobe_frame_timestamps(input)?;
    let timestamps_path = frames_dir.join("timestamps.txt");
    let timestamps_text = timestamps
        .iter()
        .map(|timestamp| format!("{timestamp:.9}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&timestamps_path, format!("{timestamps_text}\n")).map_err(|err| {
        HelioFrameError::Config(format!(
            "failed to write timestamps file {}: {err}",
            timestamps_path.display()
        ))
    })?;

    let frame_pattern = frames_dir.join("frame_%010d.png");
    run_checked(
        Command::new("ffmpeg")
            .arg("-y")
            .arg("-v")
            .arg("error")
            .arg("-i")
            .arg(input)
            .arg("-an")
            .arg("-vsync")
            .arg("0")
            .arg("-start_number")
            .arg("0")
            .arg(&frame_pattern),
        "decode frames",
    )?;

    let audio_path = if plan.preserve_audio && probe.has_audio {
        extract_audio_track(input, frames_dir)?
    } else {
        None
    };

    Ok(DecodedFrames {
        frames_dir: frames_dir.to_path_buf(),
        frame_pattern: "frame_%010d.png".to_string(),
        timestamps_path,
        frame_count: timestamps.len(),
        fps: probe.fps,
        duration_seconds: probe.duration_seconds,
        audio_path,
    })
}

fn ffprobe_frame_timestamps(input: &Path) -> HelioFrameResult<Vec<f64>> {
    let output = Command::new("ffprobe")
        .env("LC_ALL", "C")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("frame=best_effort_timestamp_time")
        .arg("-of")
        .arg("csv=p=0")
        .arg(input)
        .output()
        .map_err(|err| HelioFrameError::Config(format!("failed to execute ffprobe: {err}")))?;

    if !output.status.success() {
        return Err(HelioFrameError::Config(format!(
            "ffprobe failed while reading frame timestamps: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut timestamps = Vec::new();

    for line in stdout.lines() {
        let value = line.trim().trim_end_matches(',');
        if value.is_empty() || value == "N/A" {
            continue;
        }
        let parsed = value.parse::<f64>().map_err(|err| {
            HelioFrameError::Config(format!("invalid frame timestamp '{value}': {err}"))
        })?;
        if parsed.is_finite() {
            timestamps.push(parsed);
        }
    }

    if timestamps.is_empty() {
        return Err(HelioFrameError::Config(
            "no decodable frame timestamps found".to_string(),
        ));
    }

    Ok(timestamps)
}

fn extract_audio_track(input: &Path, frames_dir: &Path) -> HelioFrameResult<Option<PathBuf>> {
    let copied_audio = frames_dir.join("audio.mka");
    let copy_attempt = Command::new("ffmpeg")
        .arg("-y")
        .arg("-v")
        .arg("error")
        .arg("-i")
        .arg(input)
        .arg("-vn")
        .arg("-map")
        .arg("0:a:0")
        .arg("-c:a")
        .arg("copy")
        .arg(&copied_audio)
        .output()
        .map_err(|err| HelioFrameError::Config(format!("failed to execute ffmpeg: {err}")))?;

    if copy_attempt.status.success() {
        return Ok(Some(copied_audio));
    }

    let transcoded_audio = frames_dir.join("audio.m4a");
    run_checked(
        Command::new("ffmpeg")
            .arg("-y")
            .arg("-v")
            .arg("error")
            .arg("-i")
            .arg(input)
            .arg("-vn")
            .arg("-map")
            .arg("0:a:0")
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("192k")
            .arg(&transcoded_audio),
        "extract audio",
    )?;

    Ok(Some(transcoded_audio))
}

fn run_checked(command: &mut Command, stage: &str) -> HelioFrameResult<()> {
    let output = command.output().map_err(|err| {
        HelioFrameError::Config(format!("failed to execute ffmpeg for {stage}: {err}"))
    })?;

    if output.status.success() {
        return Ok(());
    }

    Err(HelioFrameError::Config(format!(
        "ffmpeg failed during {stage}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}
