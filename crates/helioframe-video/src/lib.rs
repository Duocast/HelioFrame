pub mod decoder;
pub mod encoder;
pub mod probe;
pub mod stitch;

pub use decoder::{decode_to_frame_directory, DecodePlan, DecodedFrames};
pub use encoder::{
    auto_detect_codec, encode_from_frame_directory, select_best_codec, EncodePlan, EncodeResult,
    NvencPreset, VideoCodec,
};
pub use probe::{probe_input, VideoProbe};
pub use stitch::{stitch_tiles, FrameTile, StitchPlan, StitchResult};

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use helioframe_core::Resolution;

    use crate::{
        decode_to_frame_directory, encode_from_frame_directory, probe_input, DecodePlan, EncodePlan,
        NvencPreset, VideoCodec,
    };

    #[test]
    fn decodes_and_reencodes_fixture_with_audio_sync() {
        if !ffmpeg_available() || !ffprobe_available() {
            eprintln!("skipping decode/encode test because ffmpeg/ffprobe are unavailable");
            return;
        }

        let temp_root = std::env::temp_dir().join(format!(
            "helioframe-video-roundtrip-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_root).expect("temp root should be creatable");

        let input = temp_root.join("input.mp4");
        let decode_dir = temp_root.join("frames");
        let output = temp_root.join("roundtrip.mp4");

        create_fixture_clip(&input);

        let decoded = decode_to_frame_directory(&input, &decode_dir, &DecodePlan::default())
            .expect("decode should succeed");
        assert!(decoded.frame_count > 0);
        assert!(decoded.audio_path.is_some());

        let plan = EncodePlan {
            output_resolution: Resolution {
                width: 160,
                height: 90,
            },
            preserve_audio: true,
            container_hint: "mp4",
            deterministic_output: false,
            enable_mild_denoise: false,
            resize_filter: "lanczos",
            sharpen_amount: None,
            codec: Some(VideoCodec::H264),
            nvenc_preset: NvencPreset::default(),
            quality: None,
            gpu_index: None,
            allow_10bit: false,
        };

        let encoded =
            encode_from_frame_directory(&decoded, &output, &plan).expect("encode should succeed");
        assert!(encoded.output_path.exists());

        let source_probe = probe_input(&input).expect("source should probe");
        let output_probe = probe_input(&encoded.output_path).expect("output should probe");

        let duration_delta = (source_probe.duration_seconds - output_probe.duration_seconds).abs();
        assert!(
            duration_delta < 0.12,
            "duration drift too high: {duration_delta}"
        );
        assert!(output_probe.has_audio, "output should preserve audio");

        std::fs::remove_dir_all(&temp_root).expect("cleanup should succeed");
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

    fn create_fixture_clip(path: &std::path::Path) {
        let output = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("testsrc=size=160x90:rate=24:duration=1.4")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("sine=frequency=880:sample_rate=48000:duration=1.4")
            .arg("-shortest")
            .arg("-c:v")
            .arg("libx264")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-c:a")
            .arg("aac")
            .arg(path)
            .output()
            .expect("ffmpeg invocation should execute");

        assert!(
            output.status.success(),
            "ffmpeg fixture generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
