use std::{path::Path, process::Command};

use helioframe_core::{HelioFrameError, HelioFrameResult, Resolution, VideoContainer};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct VideoProbe {
    pub container: VideoContainer,
    pub assumed_resolution: Resolution,
    pub fps: f64,
    pub duration_seconds: f64,
    pub video_codec: String,
    pub has_audio: bool,
    pub pixel_format: Option<String>,
    pub colorspace: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
    pix_fmt: Option<String>,
    color_space: Option<String>,
}

pub fn probe_input(path: &Path) -> HelioFrameResult<VideoProbe> {
    let container = VideoContainer::from_path(path)?;
    let ffprobe = run_ffprobe(path)?;

    let video_stream = ffprobe
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| HelioFrameError::Config("ffprobe found no video stream".to_string()))?;

    let width = video_stream
        .width
        .ok_or_else(|| HelioFrameError::Config("ffprobe video stream missing width".to_string()))?;
    let height = video_stream.height.ok_or_else(|| {
        HelioFrameError::Config("ffprobe video stream missing height".to_string())
    })?;

    let fps = video_stream
        .avg_frame_rate
        .as_deref()
        .and_then(parse_ffprobe_rate)
        .or_else(|| {
            video_stream
                .r_frame_rate
                .as_deref()
                .and_then(parse_ffprobe_rate)
        })
        .ok_or_else(|| HelioFrameError::Config("ffprobe stream missing frame rate".to_string()))?;

    let duration_seconds = video_stream
        .duration
        .as_deref()
        .and_then(parse_ffprobe_number)
        .or_else(|| {
            ffprobe
                .format
                .as_ref()
                .and_then(|format| format.duration.as_deref())
                .and_then(parse_ffprobe_number)
        })
        .ok_or_else(|| HelioFrameError::Config("ffprobe missing duration".to_string()))?;

    let has_audio = ffprobe
        .streams
        .iter()
        .any(|stream| stream.codec_type.as_deref() == Some("audio"));

    let video_codec = video_stream
        .codec_name
        .clone()
        .ok_or_else(|| HelioFrameError::Config("ffprobe video stream missing codec".to_string()))?;

    Ok(VideoProbe {
        container,
        assumed_resolution: Resolution { width, height },
        fps,
        duration_seconds,
        video_codec,
        has_audio,
        pixel_format: video_stream.pix_fmt.clone(),
        colorspace: video_stream.color_space.clone(),
    })
}

fn run_ffprobe(path: &Path) -> HelioFrameResult<FfprobeOutput> {
    let output = Command::new("ffprobe")
        .env("LC_ALL", "C")
        .arg("-v")
        .arg("error")
        .arg("-show_streams")
        .arg("-show_format")
        .arg("-print_format")
        .arg("json")
        .arg(path)
        .output()
        .map_err(|err| HelioFrameError::Config(format!("failed to execute ffprobe: {err}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            "ffprobe failed without stderr output".to_string()
        } else {
            stderr
        };
        return Err(HelioFrameError::Config(format!(
            "ffprobe failed for {}: {detail}",
            path.display()
        )));
    }

    serde_json::from_slice::<FfprobeOutput>(&output.stdout)
        .map_err(|err| HelioFrameError::Config(format!("invalid ffprobe json: {err}")))
}

fn parse_ffprobe_rate(value: &str) -> Option<f64> {
    if value == "0/0" {
        return None;
    }

    if let Some((numerator, denominator)) = value.split_once('/') {
        let numerator = numerator.parse::<f64>().ok()?;
        let denominator = denominator.parse::<f64>().ok()?;
        if denominator == 0.0 {
            return None;
        }
        return Some(numerator / denominator);
    }

    parse_ffprobe_number(value)
}

fn parse_ffprobe_number(value: &str) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

#[cfg(test)]
mod tests {
    use super::{parse_ffprobe_number, parse_ffprobe_rate};

    #[test]
    fn parses_fractional_frame_rate() {
        let parsed = parse_ffprobe_rate("30000/1001").expect("expected parsed rate");
        assert!((parsed - 29.97002997).abs() < 0.0001);
    }

    #[test]
    fn ignores_invalid_frame_rate() {
        assert_eq!(parse_ffprobe_rate("0/0"), None);
    }

    #[test]
    fn parses_numeric_values() {
        assert_eq!(parse_ffprobe_number("42.5"), Some(42.5));
        assert_eq!(parse_ffprobe_number("nan"), None);
    }

    #[test]
    fn probes_real_metadata_from_generated_fixture() {
        if !ffmpeg_available() || !ffprobe_available() {
            eprintln!("skipping fixture probe test because ffmpeg/ffprobe are unavailable");
            return;
        }

        let fixture = ensure_fixture_clip("probe-sample-with-audio.mp4", true);
        let probe = super::probe_input(&fixture).expect("fixture should probe successfully");

        assert_eq!(probe.assumed_resolution.width, 160);
        assert_eq!(probe.assumed_resolution.height, 90);
        assert!((probe.fps - 24.0).abs() < 0.1);
        assert!(probe.duration_seconds > 0.9);
        assert_eq!(probe.video_codec, "h264");
        assert!(probe.has_audio);
        assert_eq!(probe.pixel_format.as_deref(), Some("yuv420p"));
    }

    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn ffprobe_available() -> bool {
        std::process::Command::new("ffprobe")
            .arg("-version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn ensure_fixture_clip(name: &str, with_audio: bool) -> std::path::PathBuf {
        let fixture_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("tests")
            .join("fixtures");
        std::fs::create_dir_all(&fixture_dir).expect("fixture directory should be creatable");

        let path = fixture_dir.join(name);
        if path.exists() {
            return path;
        }

        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("testsrc=size=160x90:rate=24:duration=1")
            .arg("-pix_fmt")
            .arg("yuv420p");

        if with_audio {
            cmd.arg("-f")
                .arg("lavfi")
                .arg("-i")
                .arg("sine=frequency=1000:sample_rate=48000:duration=1")
                .arg("-shortest")
                .arg("-c:a")
                .arg("aac");
        } else {
            cmd.arg("-an");
        }

        let output = cmd
            .arg("-c:v")
            .arg("libx264")
            .arg(&path)
            .output()
            .expect("ffmpeg should run to generate fixture");

        assert!(
            output.status.success(),
            "ffmpeg failed while generating fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        path
    }
}
