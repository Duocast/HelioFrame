use std::{
    fmt,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use helioframe_core::{
    detect_nvenc_capabilities, HelioFrameError, HelioFrameResult, NvencCapabilities, Resolution,
};

use crate::DecodedFrames;

/// Video encoder codec selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    /// Software H.264 via libx264 (always available).
    H264,
    /// NVIDIA NVENC H.264.
    H264Nvenc,
    /// NVIDIA NVENC H.265 / HEVC.
    HevcNvenc,
    /// NVIDIA NVENC AV1 (Ada Lovelace / Blackwell and later).
    Av1Nvenc,
}

impl VideoCodec {
    /// Returns the FFmpeg encoder name for this codec.
    pub fn ffmpeg_encoder_name(self) -> &'static str {
        match self {
            Self::H264 => "libx264",
            Self::H264Nvenc => "h264_nvenc",
            Self::HevcNvenc => "hevc_nvenc",
            Self::Av1Nvenc => "av1_nvenc",
        }
    }

    /// Returns `true` when this codec uses NVENC hardware encoding.
    pub fn is_nvenc(self) -> bool {
        matches!(self, Self::H264Nvenc | Self::HevcNvenc | Self::Av1Nvenc)
    }

    /// Returns the software fallback codec for this encoder.
    pub fn software_fallback(self) -> Self {
        Self::H264
    }
}

impl fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ffmpeg_encoder_name())
    }
}

/// Select the best available encoder given detected NVENC capabilities and a
/// user preference.
///
/// When `preferred` is `None`, the function picks the best NVENC encoder
/// available (preferring HEVC > H.264 > AV1 for quality-at-bitrate), falling
/// back to software H.264.
///
/// When `preferred` is `Some(codec)`, it validates that the codec is available,
/// returning `H264` as fallback if the requested hardware encoder is absent.
pub fn select_best_codec(
    nvenc: &NvencCapabilities,
    preferred: Option<VideoCodec>,
) -> VideoCodec {
    if let Some(codec) = preferred {
        if !codec.is_nvenc() {
            return codec;
        }
        let available = match codec {
            VideoCodec::H264Nvenc => nvenc.h264_nvenc,
            VideoCodec::HevcNvenc => nvenc.hevc_nvenc,
            VideoCodec::Av1Nvenc => nvenc.av1_nvenc,
            _ => false,
        };
        if available {
            return codec;
        }
        return codec.software_fallback();
    }

    // Auto-select: prefer HEVC NVENC for quality, then H.264 NVENC for
    // compatibility, then AV1 NVENC, finally software H.264.
    if nvenc.hevc_nvenc {
        VideoCodec::HevcNvenc
    } else if nvenc.h264_nvenc {
        VideoCodec::H264Nvenc
    } else if nvenc.av1_nvenc {
        VideoCodec::Av1Nvenc
    } else {
        VideoCodec::H264
    }
}

/// Detect available NVENC encoders and pick the best one automatically.
pub fn auto_detect_codec() -> (VideoCodec, NvencCapabilities) {
    let caps = detect_nvenc_capabilities();
    let codec = select_best_codec(&caps, None);
    (codec, caps)
}

/// NVENC quality/rate-control preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NvencPreset {
    /// Fastest, lowest quality (NVENC p1).
    P1,
    /// Good balance of speed and quality (NVENC p4).
    P4,
    /// High quality (NVENC p5).
    P5,
    /// Highest quality (NVENC p7).
    P7,
}

impl NvencPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::P1 => "p1",
            Self::P4 => "p4",
            Self::P5 => "p5",
            Self::P7 => "p7",
        }
    }
}

impl Default for NvencPreset {
    fn default() -> Self {
        Self::P5
    }
}

#[derive(Debug, Clone)]
pub struct EncodePlan {
    pub output_resolution: Resolution,
    pub preserve_audio: bool,
    pub container_hint: &'static str,
    pub deterministic_output: bool,
    pub enable_mild_denoise: bool,
    pub resize_filter: &'static str,
    pub sharpen_amount: Option<f32>,
    /// Which video codec/encoder to use. `None` means auto-detect at encode
    /// time (picks NVENC if available, software fallback otherwise).
    pub codec: Option<VideoCodec>,
    /// NVENC quality preset (ignored for software codecs).
    pub nvenc_preset: NvencPreset,
    /// Constant-quality parameter. For NVENC this maps to `-cq`, for libx264
    /// it maps to `-crf`. Range 0-51, lower = higher quality. `None` uses
    /// the encoder default.
    pub quality: Option<u8>,
    /// Optional GPU device index for NVENC (`-gpu N`). `None` uses the
    /// default device.
    pub gpu_index: Option<u32>,
    /// Allow 10-bit pixel output when the codec supports it (HEVC/AV1 NVENC).
    pub allow_10bit: bool,
}

impl Default for EncodePlan {
    fn default() -> Self {
        Self {
            output_resolution: Resolution::UHD_4K,
            preserve_audio: true,
            container_hint: "mp4",
            deterministic_output: false,
            enable_mild_denoise: false,
            resize_filter: "lanczos",
            sharpen_amount: None,
            codec: None,
            nvenc_preset: NvencPreset::default(),
            quality: None,
            gpu_index: None,
            allow_10bit: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EncodeResult {
    pub output_path: PathBuf,
    /// The codec that was actually used (may differ from the plan if fallback
    /// was triggered).
    pub codec_used: VideoCodec,
}

pub fn encode_from_frame_directory(
    decoded: &DecodedFrames,
    output_path: &Path,
    plan: &EncodePlan,
) -> HelioFrameResult<EncodeResult> {
    let concat_path = decoded.frames_dir.join("frames.ffconcat");
    let frame_paths = gather_frame_paths(decoded)?;

    write_ffconcat_file(&concat_path, &frame_paths, &decoded.timestamps_path)?;

    // Resolve the actual codec to use.
    let codec = match plan.codec {
        Some(c) => c,
        None => {
            let (auto_codec, _caps) = auto_detect_codec();
            auto_codec
        }
    };

    if plan.preserve_audio {
        if let Some(audio_path) = decoded.audio_path.as_ref() {
            // Try the selected codec first.
            let output =
                run_encode(Some(audio_path), &concat_path, output_path, plan, true, codec)?;
            if output.status.success() {
                return Ok(EncodeResult {
                    output_path: output_path.to_path_buf(),
                    codec_used: codec,
                });
            }

            // If NVENC failed, fall back to software before giving up on audio copy.
            if codec.is_nvenc() {
                let fallback_codec = codec.software_fallback();
                let fb_output = run_encode(
                    Some(audio_path),
                    &concat_path,
                    output_path,
                    plan,
                    true,
                    fallback_codec,
                )?;
                if fb_output.status.success() {
                    return Ok(EncodeResult {
                        output_path: output_path.to_path_buf(),
                        codec_used: fallback_codec,
                    });
                }
            }

            // Try transcoded audio with the primary codec.
            let fallback =
                run_encode(Some(audio_path), &concat_path, output_path, plan, false, codec)?;
            if fallback.status.success() {
                return Ok(EncodeResult {
                    output_path: output_path.to_path_buf(),
                    codec_used: codec,
                });
            }

            // Final fallback: transcoded audio with software codec.
            if codec.is_nvenc() {
                let fallback_codec = codec.software_fallback();
                let fb_output = run_encode(
                    Some(audio_path),
                    &concat_path,
                    output_path,
                    plan,
                    false,
                    fallback_codec,
                )?;
                if fb_output.status.success() {
                    return Ok(EncodeResult {
                        output_path: output_path.to_path_buf(),
                        codec_used: fallback_codec,
                    });
                }
            }

            return Err(HelioFrameError::Config(format!(
                "ffmpeg encode failed with codec {} (and software fallback): {}",
                codec,
                String::from_utf8_lossy(&fallback.stderr).trim(),
            )));
        }
    }

    // No audio path.
    let no_audio = run_encode(None, &concat_path, output_path, plan, false, codec)?;
    if no_audio.status.success() {
        return Ok(EncodeResult {
            output_path: output_path.to_path_buf(),
            codec_used: codec,
        });
    }

    // NVENC no-audio fallback to software.
    if codec.is_nvenc() {
        let fallback_codec = codec.software_fallback();
        let fb_output =
            run_encode(None, &concat_path, output_path, plan, false, fallback_codec)?;
        if fb_output.status.success() {
            return Ok(EncodeResult {
                output_path: output_path.to_path_buf(),
                codec_used: fallback_codec,
            });
        }
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
    codec: VideoCodec,
) -> HelioFrameResult<Output> {
    let mut filters = Vec::new();
    if plan.enable_mild_denoise {
        filters.push("hqdn3d=1.5:1.5:6:6".to_string());
    }
    filters.push(format!(
        "zscale=w={}:h={}:filter={}",
        plan.output_resolution.width, plan.output_resolution.height, plan.resize_filter
    ));
    if let Some(sharpen_amount) = plan.sharpen_amount {
        if sharpen_amount > 0.0 {
            filters.push(format!("unsharp=5:5:{sharpen_amount:.3}:5:5:0.000"));
        }
    }

    // For NVENC with 10-bit, convert pixel format inside the filter chain.
    let pix_fmt = resolve_pixel_format(codec, plan.allow_10bit);
    if pix_fmt == "p010le" {
        filters.push("format=p010le".to_string());
    }

    let filter_chain = filters.join(",");

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

    command.arg("-vf").arg(filter_chain);

    // --- Codec-specific arguments ---
    command.arg("-c:v").arg(codec.ffmpeg_encoder_name());

    match codec {
        VideoCodec::H264 => {
            apply_libx264_args(&mut command, plan);
        }
        VideoCodec::H264Nvenc => {
            apply_nvenc_args(&mut command, plan, "high", pix_fmt);
        }
        VideoCodec::HevcNvenc => {
            let profile = if pix_fmt == "p010le" { "main10" } else { "main" };
            apply_nvenc_args(&mut command, plan, profile, pix_fmt);
        }
        VideoCodec::Av1Nvenc => {
            apply_nvenc_args(&mut command, plan, "main", pix_fmt);
        }
    }

    command
        .arg("-fps_mode:v")
        .arg("passthrough")
        .arg("-movflags")
        .arg("+faststart");

    if plan.deterministic_output {
        command.arg("-threads").arg("1");
        command.arg("-fflags").arg("+bitexact");
        command.arg("-flags:v").arg("+bitexact");
        command.arg("-flags:a").arg("+bitexact");
    }

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

/// Determine pixel format for the selected codec.
fn resolve_pixel_format(codec: VideoCodec, allow_10bit: bool) -> &'static str {
    if allow_10bit {
        match codec {
            VideoCodec::HevcNvenc | VideoCodec::Av1Nvenc => "p010le",
            _ => "yuv420p",
        }
    } else {
        "yuv420p"
    }
}

/// Apply libx264-specific encoding arguments.
fn apply_libx264_args(command: &mut Command, plan: &EncodePlan) {
    command.arg("-pix_fmt").arg("yuv420p");
    if let Some(crf) = plan.quality {
        command.arg("-crf").arg(crf.to_string());
    }
}

/// Apply NVENC-specific encoding arguments.
fn apply_nvenc_args(
    command: &mut Command,
    plan: &EncodePlan,
    profile: &str,
    pix_fmt: &str,
) {
    if pix_fmt != "p010le" {
        command.arg("-pix_fmt").arg("yuv420p");
    }

    command.arg("-preset").arg(plan.nvenc_preset.as_str());
    command.arg("-profile:v").arg(profile);
    command.arg("-tune").arg("hq");
    command.arg("-rc").arg("vbr");

    if let Some(cq) = plan.quality {
        command.arg("-cq").arg(cq.to_string());
    } else {
        // Sensible default for 4K quality-oriented output.
        command.arg("-cq").arg("20");
    }

    command.arg("-b:v").arg("0");

    if let Some(gpu_idx) = plan.gpu_index {
        command.arg("-gpu").arg(gpu_idx.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helioframe_core::NvencCapabilities;

    #[test]
    fn auto_select_prefers_hevc_nvenc_when_available() {
        let caps = NvencCapabilities {
            h264_nvenc: true,
            hevc_nvenc: true,
            av1_nvenc: false,
            detail: String::new(),
        };
        assert_eq!(select_best_codec(&caps, None), VideoCodec::HevcNvenc);
    }

    #[test]
    fn auto_select_falls_back_to_h264_nvenc() {
        let caps = NvencCapabilities {
            h264_nvenc: true,
            hevc_nvenc: false,
            av1_nvenc: false,
            detail: String::new(),
        };
        assert_eq!(select_best_codec(&caps, None), VideoCodec::H264Nvenc);
    }

    #[test]
    fn auto_select_falls_back_to_software_when_no_nvenc() {
        let caps = NvencCapabilities {
            h264_nvenc: false,
            hevc_nvenc: false,
            av1_nvenc: false,
            detail: String::new(),
        };
        assert_eq!(select_best_codec(&caps, None), VideoCodec::H264);
    }

    #[test]
    fn preferred_codec_validated_against_capabilities() {
        let caps = NvencCapabilities {
            h264_nvenc: true,
            hevc_nvenc: false,
            av1_nvenc: false,
            detail: String::new(),
        };
        // Requesting HEVC but it's not available -> falls back to software.
        assert_eq!(
            select_best_codec(&caps, Some(VideoCodec::HevcNvenc)),
            VideoCodec::H264
        );
        // Requesting H264 NVENC which IS available.
        assert_eq!(
            select_best_codec(&caps, Some(VideoCodec::H264Nvenc)),
            VideoCodec::H264Nvenc
        );
    }

    #[test]
    fn software_codec_always_accepted() {
        let caps = NvencCapabilities::default();
        assert_eq!(
            select_best_codec(&caps, Some(VideoCodec::H264)),
            VideoCodec::H264
        );
    }

    #[test]
    fn resolve_pixel_format_10bit() {
        assert_eq!(resolve_pixel_format(VideoCodec::HevcNvenc, true), "p010le");
        assert_eq!(resolve_pixel_format(VideoCodec::H264Nvenc, true), "yuv420p");
        assert_eq!(resolve_pixel_format(VideoCodec::HevcNvenc, false), "yuv420p");
    }

    #[test]
    fn nvenc_preset_defaults_to_p5() {
        assert_eq!(NvencPreset::default(), NvencPreset::P5);
    }

    #[test]
    fn video_codec_display_and_properties() {
        assert_eq!(VideoCodec::H264.ffmpeg_encoder_name(), "libx264");
        assert_eq!(VideoCodec::H264Nvenc.ffmpeg_encoder_name(), "h264_nvenc");
        assert_eq!(VideoCodec::HevcNvenc.ffmpeg_encoder_name(), "hevc_nvenc");
        assert_eq!(VideoCodec::Av1Nvenc.ffmpeg_encoder_name(), "av1_nvenc");
        assert!(!VideoCodec::H264.is_nvenc());
        assert!(VideoCodec::H264Nvenc.is_nvenc());
        assert!(VideoCodec::HevcNvenc.is_nvenc());
        assert!(VideoCodec::Av1Nvenc.is_nvenc());
    }
}
