use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use helioframe_core::Resolution;
use helioframe_video::{
    decode_to_frame_directory, encode_from_frame_directory, probe_input, DecodePlan, EncodePlan,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FixtureClipRecipe {
    name: String,
    file: String,
    container: String,
    width: u32,
    height: u32,
    fps: u32,
    duration_seconds: f64,
    expected_frames: usize,
    has_audio: bool,
    video_filter: String,
}

#[derive(Debug, Deserialize)]
struct BaselineConfig {
    roundtrip: RoundtripBaseline,
}

#[derive(Debug, Deserialize)]
struct RoundtripBaseline {
    container: String,
    pixel_format: String,
    min_duration_seconds: f64,
    max_duration_seconds: f64,
    max_frame_delta: usize,
    fps_tolerance: f64,
}

#[test]
fn probe_metadata_and_frame_counts_match_fixture_recipes() {
    if !ffmpeg_available() || !ffprobe_available() {
        eprintln!("skipping probe metadata regression because ffmpeg/ffprobe are unavailable");
        return;
    }

    let fixtures = load_fixture_recipes();
    assert!(
        (5..=10).contains(&fixtures.len()),
        "expected 5-10 fixture recipes, got {}",
        fixtures.len()
    );

    let fixture_dir = fixtures_dir();
    for recipe in &fixtures {
        let path = ensure_fixture_clip(&fixture_dir, recipe);
        let probe = probe_input(&path).expect("fixture should probe successfully");
        let decoded_frame_count = ffprobe_video_frame_count(&path).expect("frame count should parse");

        assert_eq!(probe.container.to_string(), recipe.container);
        assert_eq!(probe.assumed_resolution.width, recipe.width, "{}", recipe.name);
        assert_eq!(probe.assumed_resolution.height, recipe.height, "{}", recipe.name);
        assert!(
            (probe.fps - f64::from(recipe.fps)).abs() < 0.2,
            "{} fps mismatch: expected {}, got {}",
            recipe.name,
            recipe.fps,
            probe.fps
        );
        assert_eq!(probe.has_audio, recipe.has_audio, "{}", recipe.name);
        assert_eq!(decoded_frame_count, recipe.expected_frames, "{}", recipe.name);
        assert!(
            (probe.duration_seconds - recipe.duration_seconds).abs() < 0.15,
            "{} duration mismatch: expected {}, got {}",
            recipe.name,
            recipe.duration_seconds,
            probe.duration_seconds
        );
    }
}

#[test]
fn decode_encode_roundtrip_outputs_exist_and_satisfy_baseline_metadata() {
    if !ffmpeg_available() || !ffprobe_available() {
        eprintln!("skipping roundtrip regression because ffmpeg/ffprobe are unavailable");
        return;
    }

    let fixtures = load_fixture_recipes();
    let baseline = load_baseline();
    let fixture_dir = fixtures_dir();
    let run_root = make_temp_root("helioframe-video-io-regression");

    for recipe in &fixtures {
        let input = ensure_fixture_clip(&fixture_dir, recipe);
        let decode_dir = run_root.join(&recipe.name).join("decoded");
        fs::create_dir_all(&decode_dir).expect("decode dir should be creatable");

        let decoded = decode_to_frame_directory(&input, &decode_dir, &DecodePlan::default())
            .expect("decode should succeed");
        assert_eq!(decoded.frame_count, recipe.expected_frames, "{}", recipe.name);

        let output = run_root
            .join(&recipe.name)
            .join(format!("{}_roundtrip.{}", recipe.name, baseline.roundtrip.container));

        let plan = EncodePlan {
            output_resolution: Resolution {
                width: recipe.width,
                height: recipe.height,
            },
            preserve_audio: true,
            container_hint: "mp4",
            deterministic_output: true,
            enable_mild_denoise: false,
            resize_filter: "lanczos",
            sharpen_amount: None,
            codec: Some(helioframe_video::VideoCodec::H264),
            nvenc_preset: helioframe_video::NvencPreset::default(),
            quality: None,
            gpu_index: None,
            allow_10bit: false,
        };

        let encoded =
            encode_from_frame_directory(&decoded, &output, &plan).expect("encode should succeed");
        assert!(encoded.output_path.exists(), "{}", recipe.name);

        let output_probe = probe_input(&encoded.output_path).expect("output should probe");
        let output_frame_count =
            ffprobe_video_frame_count(&encoded.output_path).expect("output frame count should parse");

        let frame_delta = output_frame_count.abs_diff(decoded.frame_count);
        assert!(
            frame_delta <= baseline.roundtrip.max_frame_delta,
            "{} frame delta too high: decoded={} output={}",
            recipe.name,
            decoded.frame_count,
            output_frame_count
        );
        assert_eq!(
            output_probe.pixel_format.as_deref(),
            Some(baseline.roundtrip.pixel_format.as_str()),
            "{}",
            recipe.name
        );
        assert!(
            (output_probe.fps - decoded.fps).abs() <= baseline.roundtrip.fps_tolerance,
            "{} fps drift too high: decoded={} output={}",
            recipe.name,
            decoded.fps,
            output_probe.fps
        );
        assert!(
            output_probe.duration_seconds >= baseline.roundtrip.min_duration_seconds
                && output_probe.duration_seconds <= baseline.roundtrip.max_duration_seconds,
            "{} duration outside baseline envelope: {}",
            recipe.name,
            output_probe.duration_seconds
        );

        if recipe.has_audio {
            assert!(output_probe.has_audio, "{} should preserve audio", recipe.name);
        }
    }

    fs::remove_dir_all(run_root).expect("cleanup should succeed");
}

fn load_fixture_recipes() -> Vec<FixtureClipRecipe> {
    let path = repo_root().join("tests").join("fixtures").join("clip_recipes.json");
    let text = fs::read_to_string(path).expect("fixture recipe json should be readable");
    serde_json::from_str(&text).expect("fixture recipe json should parse")
}

fn load_baseline() -> BaselineConfig {
    let path = repo_root()
        .join("tests")
        .join("integration")
        .join("baseline_expectations.json");
    let text = fs::read_to_string(path).expect("baseline json should be readable");
    serde_json::from_str(&text).expect("baseline json should parse")
}

fn fixtures_dir() -> PathBuf {
    repo_root().join("tests").join("fixtures").join("clips")
}

fn ensure_fixture_clip(fixtures_dir: &Path, recipe: &FixtureClipRecipe) -> PathBuf {
    fs::create_dir_all(fixtures_dir).expect("fixture dir should be creatable");
    let path = fixtures_dir.join(&recipe.file);
    if path.exists() {
        return path;
    }

    let video_input = format!(
        "{}=size={}x{}:rate={}:duration={}",
        recipe.video_filter, recipe.width, recipe.height, recipe.fps, recipe.duration_seconds
    );

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .arg("-v")
        .arg("error")
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg(video_input)
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-threads")
        .arg("1")
        .arg("-fflags")
        .arg("+bitexact")
        .arg("-flags:v")
        .arg("+bitexact")
        .arg("-c:v")
        .arg("libx264");

    if recipe.has_audio {
        let audio_input = format!(
            "sine=frequency=440:sample_rate=48000:duration={}",
            recipe.duration_seconds
        );
        cmd.arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg(audio_input)
            .arg("-shortest")
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("96k");
    } else {
        cmd.arg("-an");
    }

    let output = cmd.arg(&path).output().expect("ffmpeg should run");
    assert!(
        output.status.success(),
        "failed to generate fixture {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );

    path
}

fn ffprobe_video_frame_count(path: &Path) -> Result<usize, String> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-count_frames")
        .arg("-show_entries")
        .arg("stream=nb_read_frames")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()
        .map_err(|err| format!("failed to execute ffprobe: {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    raw.trim()
        .parse::<usize>()
        .map_err(|err| format!("invalid frame count '{}': {err}", raw.trim()))
}

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn ffprobe_available() -> bool {
    Command::new("ffprobe")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root should canonicalize")
}

fn make_temp_root(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
    fs::create_dir_all(&path).expect("temp root should be creatable");
    path
}
