use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use helioframe_core::{
    AppConfig, HelioFrameResult, PresetConfig, RunLayout, RunManifest, RunProbeInfo,
};
use helioframe_model::{BackendRegistry, InferencePlan, WorkerLaunchConfig};
use helioframe_video::{
    decode_to_frame_directory, encode_from_frame_directory, probe_input, DecodePlan, EncodePlan,
    VideoProbe,
};

use crate::shots::{detect_shots, DEFAULT_SCDET_THRESHOLD};
use crate::stages::PipelineStage;
use crate::tiling::build_window_tile_manifests;
use crate::windows::build_windows_and_batches;

const WINDOWS_ARTIFACT_FILENAME: &str = "windows.json";
const ANCHORS_ARTIFACT_FILENAME: &str = "anchors.json";

#[derive(Debug, serde::Serialize)]
struct ProbeArtifact {
    container: String,
    assumed_resolution: String,
    fps: f64,
    duration_seconds: f64,
    video_codec: String,
    has_audio: bool,
    pixel_format: Option<String>,
    colorspace: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub probe: VideoProbe,
    pub preset: PresetConfig,
    pub decode: DecodePlan,
    pub inference: InferencePlan,
    pub encode: EncodePlan,
    pub stages: Vec<PipelineStage>,
}

#[derive(Debug, Clone)]
pub struct RunExecution {
    pub plan: ExecutionPlan,
    pub run_layout: RunLayout,
}

pub struct PipelineOrchestrator;

impl PipelineOrchestrator {
    fn write_stage_artifact<T: serde::Serialize>(
        run_layout: &RunLayout,
        filename: &str,
        value: &T,
        artifact_label: &str,
    ) -> HelioFrameResult<()> {
        let artifact_path = run_layout.intermediate_artifacts_dir.join(filename);
        let artifact_json = serde_json::to_string_pretty(value).map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to serialize {artifact_label} artifact: {err}"
            ))
        })?;

        fs::write(&artifact_path, artifact_json).map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to write {artifact_label} artifact {}: {err}",
                artifact_path.display()
            ))
        })?;

        Ok(())
    }

    pub fn plan(config: &AppConfig, preset: PresetConfig) -> HelioFrameResult<ExecutionPlan> {
        config.validate()?;
        preset.validate_selection(config.preset, config.backend)?;
        let probe = probe_input(Path::new(&config.input))?;
        let backend = BackendRegistry::resolve(config.backend);
        let execution_profile = backend.execution_profile();
        let inference = backend.build_plan(config.target_resolution);

        let mut stages = vec![
            PipelineStage {
                name: "probe",
                description:
                    "Inspect container, stream metadata, codec compatibility, and source resolution.",
            },
            PipelineStage {
                name: "decode",
                description: "Decode video frames and audio through FFmpeg-backed I/O.",
            },
        ];

        if execution_profile.deterministic_output {
            stages.extend([
                PipelineStage {
                    name: "restore",
                    description: "Run deterministic classical restoration before final encode.",
                },
                PipelineStage {
                    name: "encode",
                    description:
                        "Encode deterministic 4K output with zscale=lanczos and optional mild denoise.",
                },
            ]);
        } else {
            stages.extend([
                PipelineStage {
                    name: "normalize",
                    description:
                        "Normalize pixel format, transfer characteristics, colorspace, and tensor layout.",
                },
                PipelineStage {
                    name: "shots",
                    description:
                        "Detect scene boundaries so motion and guidance state do not leak across cuts.",
                },
                PipelineStage {
                    name: "window",
                    description:
                        "Build temporal windows sized for high-quality restoration rather than maximum throughput.",
                },
                PipelineStage {
                    name: "anchors",
                    description:
                        "Select anchor frames and guidance frames for structure-sensitive restoration.",
                },
                PipelineStage {
                    name: "tile",
                    description:
                        "Schedule overlap-heavy spatial patches sized for 4K reconstruction and seam suppression.",
                },
                PipelineStage {
                    name: "restore",
                    description:
                        "Run the primary restoration backend over each temporal window and patch batch.",
                },
            ]);

            if preset.enable_detail_refiner || inference.hints.detail_refiner {
                stages.push(PipelineStage {
                    name: "refine",
                    description: "Apply dedicated high-frequency detail refinement to recover edges, textures, and small structures.",
                });
            }

            if preset.enable_temporal_consistency_checks || inference.hints.temporal_qc_gate {
                stages.push(PipelineStage {
                    name: "temporal-qc",
                    description: "Measure flicker, drift, and ghosting; optionally reject or rerun unstable windows.",
                });
            }

            stages.extend([
                PipelineStage {
                    name: "stitch",
                    description:
                        "Blend overlaps, reconstruct full frames, and remove tile boundary artifacts.",
                },
                PipelineStage {
                    name: "encode",
                    description: "Encode final 4K output and remux preserved audio.",
                },
            ]);
        }

        Ok(ExecutionPlan {
            probe,
            preset,
            decode: DecodePlan::default(),
            encode: EncodePlan {
                output_resolution: config.target_resolution,
                preserve_audio: true,
                container_hint: "mp4",
                deterministic_output: execution_profile.deterministic_output,
                enable_mild_denoise: execution_profile.enable_mild_denoise,
                resize_filter: execution_profile.resize_filter,
                sharpen_amount: execution_profile.sharpen_amount,
            },
            inference,
            stages,
        })
    }

    pub fn execute(config: &AppConfig, preset: PresetConfig) -> HelioFrameResult<RunExecution> {
        let base_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::execute_in_dir(config, preset, base_dir)
    }

    pub fn execute_in_dir(
        config: &AppConfig,
        preset: PresetConfig,
        base_dir: impl AsRef<Path>,
    ) -> HelioFrameResult<RunExecution> {
        let plan = Self::plan(config, preset)?;
        let backend = BackendRegistry::resolve(config.backend);
        let run_layout = RunLayout::create(base_dir)?;

        let probe = RunProbeInfo {
            container: plan.probe.container.to_string(),
            assumed_resolution: plan.probe.assumed_resolution.to_string(),
        };
        let mut manifest = RunManifest::new(run_layout.run_id.clone(), config, probe);
        run_layout.write_manifest(&manifest)?;

        let mut decoded_frames = None;
        let mut shot_detection = None;
        let mut temporal_windows = None;

        for stage in &plan.stages {
            match stage.name {
                "probe" => {
                    let started = Instant::now();
                    let probe = &plan.probe;
                    let probe_artifact = ProbeArtifact {
                        container: probe.container.to_string(),
                        assumed_resolution: probe.assumed_resolution.to_string(),
                        fps: probe.fps,
                        duration_seconds: probe.duration_seconds,
                        video_codec: probe.video_codec.clone(),
                        has_audio: probe.has_audio,
                        pixel_format: probe.pixel_format.clone(),
                        colorspace: probe.colorspace.clone(),
                    };
                    Self::write_stage_artifact(
                        &run_layout,
                        "probe.json",
                        &probe_artifact,
                        "probe",
                    )?;
                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "decode" => {
                    let started = Instant::now();
                    let decode_dir = run_layout.intermediate_artifacts_dir.join("decoded");
                    decoded_frames = Some(decode_to_frame_directory(
                        Path::new(&config.input),
                        &decode_dir,
                        &plan.decode,
                    )?);
                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "shots" => {
                    let started = Instant::now();
                    let decoded = decoded_frames.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "shots stage cannot run before decode stage".to_string(),
                        )
                    })?;

                    let detections =
                        detect_shots(Path::new(&config.input), decoded, DEFAULT_SCDET_THRESHOLD)?;

                    Self::write_stage_artifact(
                        &run_layout,
                        "shots.json",
                        &detections,
                        "shot detection",
                    )?;

                    shot_detection = Some(detections);
                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "window" => {
                    let started = Instant::now();
                    let detections = shot_detection.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "window stage cannot run before shots stage".to_string(),
                        )
                    })?;
                    let (windows, batches) = build_windows_and_batches(
                        detections.frame_count,
                        &detections.boundaries,
                        plan.preset.temporal_window,
                        plan.preset.anchor_frame_stride,
                    );
                    Self::write_stage_artifact(
                        &run_layout,
                        WINDOWS_ARTIFACT_FILENAME,
                        &windows,
                        "temporal windows",
                    )?;
                    Self::write_stage_artifact(
                        &run_layout,
                        "window_batches.json",
                        &batches,
                        "temporal window batches",
                    )?;

                    temporal_windows = Some(windows.clone());
                    manifest.set_windows(windows);
                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "anchors" => {
                    let started = Instant::now();
                    let windows = temporal_windows.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "anchors stage cannot run before window stage".to_string(),
                        )
                    })?;
                    let anchors: Vec<Vec<usize>> = windows
                        .iter()
                        .map(|window| window.anchor_frames.clone())
                        .collect();
                    Self::write_stage_artifact(
                        &run_layout,
                        ANCHORS_ARTIFACT_FILENAME,
                        &anchors,
                        "anchor frame",
                    )?;

                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "tile" => {
                    let started = Instant::now();
                    let windows = temporal_windows.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "tile stage cannot run before window stage".to_string(),
                        )
                    })?;
                    let tile_manifests = build_window_tile_manifests(
                        windows,
                        config.target_resolution.width as usize,
                        config.target_resolution.height as usize,
                        plan.preset.tile_size,
                        plan.preset.overlap,
                    );
                    Self::write_stage_artifact(
                        &run_layout,
                        "tile_manifest.json",
                        &tile_manifests,
                        "tile manifest",
                    )?;
                    manifest.set_window_tiles(tile_manifests);
                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "restore" => {
                    let started = Instant::now();
                    let decoded = decoded_frames.as_mut().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "restore stage cannot run before decode stage".to_string(),
                        )
                    })?;
                    let worker_launch = WorkerLaunchConfig::new(
                        &run_layout,
                        &manifest,
                        &decoded.frames_dir,
                        decoded.frame_count,
                        &manifest.window_tiles,
                        backend.name(),
                    );
                    let worker_result = backend.worker_adapter().run(worker_launch)?;

                    decoded.frames_dir = worker_result.output_frames_dir.clone();
                    decoded.frame_count = worker_result.frame_count;
                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "encode" => {
                    let started = Instant::now();
                    let decoded = decoded_frames.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "encode stage cannot run before decode stage".to_string(),
                        )
                    })?;
                    let output_path = PathBuf::from(&config.output);
                    if let Some(parent) = output_path.parent() {
                        if !parent.as_os_str().is_empty() {
                            fs::create_dir_all(parent).map_err(|err| {
                                helioframe_core::HelioFrameError::Config(format!(
                                    "failed to create output directory {}: {err}",
                                    parent.display()
                                ))
                            })?;
                        }
                    }

                    let encoded =
                        encode_from_frame_directory(&decoded, &output_path, &plan.encode)?;

                    let artifact_path = run_layout.output_artifacts_dir.join(
                        output_path
                            .file_name()
                            .unwrap_or_else(|| std::ffi::OsStr::new("output.mp4")),
                    );
                    fs::copy(&encoded.output_path, artifact_path).map_err(|err| {
                        helioframe_core::HelioFrameError::Config(format!(
                            "failed to copy encoded output into run artifacts: {err}"
                        ))
                    })?;
                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                _ => {
                    manifest.append_stage_timing(stage.name, std::time::Duration::from_millis(0));
                }
            }
            run_layout.write_manifest(&manifest)?;
        }

        Ok(RunExecution { plan, run_layout })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helioframe_core::{BackendKind, Resolution, UpscalePreset};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_preset() -> PresetConfig {
        PresetConfig {
            name: "studio".into(),
            default_backend: BackendKind::StcditStudio,
            allowed_backends: vec![BackendKind::StcditStudio, BackendKind::SeedvrTeacher],
            temporal_window: 20,
            tile_size: 1024,
            overlap: 96,
            diffusion_steps: 16,
            use_half_precision: true,
            enable_patchwise_4k: true,
            enable_structural_guidance: true,
            enable_detail_refiner: true,
            enable_temporal_consistency_checks: true,
            reject_on_temporal_regression: true,
            anchor_frame_stride: 4,
            notes: "test".into(),
        }
    }

    #[test]
    fn execute_writes_incremental_manifest_to_run_directory() {
        let temp = std::env::temp_dir().join(format!(
            "helioframe-orchestrator-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        if !ffmpeg_available() || !ffprobe_available() {
            eprintln!("skipping orchestrator manifest test because ffmpeg/ffprobe are unavailable");
            return;
        }

        let input = create_fixture_clip(&temp);
        let config = AppConfig {
            input: input.to_string_lossy().to_string(),
            output: "output.mp4".into(),
            backend: BackendKind::StcditStudio,
            preset: UpscalePreset::Studio,
            target_resolution: Resolution::UHD_4K,
        };

        let execution = PipelineOrchestrator::execute_in_dir(&config, sample_preset(), &temp)
            .expect("execution should create run artifacts");

        assert!(execution.run_layout.run_dir.exists());
        assert!(execution.run_layout.manifest_path.exists());

        let raw_manifest = std::fs::read_to_string(&execution.run_layout.manifest_path)
            .expect("manifest should be readable");
        let manifest: RunManifest =
            serde_json::from_str(&raw_manifest).expect("manifest should parse as json");

        assert_eq!(manifest.input, input.to_string_lossy());
        assert_eq!(manifest.output, "output.mp4");
        assert!(manifest.stage_timings.len() >= execution.plan.stages.len());
        assert!(!manifest.windows.is_empty());
        assert!(!manifest.window_tiles.is_empty());
        assert_eq!(manifest.windows[0].start_frame, 0);
        assert!(!manifest.windows[0].anchor_frames.is_empty());
        assert!(!manifest.window_tiles[0].tiles.is_empty());
        assert!(execution
            .run_layout
            .intermediate_artifacts_dir
            .join("probe.json")
            .exists());
        assert!(execution
            .run_layout
            .intermediate_artifacts_dir
            .join("shots.json")
            .exists());
        assert!(execution
            .run_layout
            .intermediate_artifacts_dir
            .join(WINDOWS_ARTIFACT_FILENAME)
            .exists());
        assert!(execution
            .run_layout
            .intermediate_artifacts_dir
            .join(ANCHORS_ARTIFACT_FILENAME)
            .exists());
        assert!(execution
            .run_layout
            .intermediate_artifacts_dir
            .join("tile_manifest.json")
            .exists());
        assert!(execution
            .run_layout
            .intermediate_artifacts_dir
            .join("window_batches.json")
            .exists());

        let windows_raw = std::fs::read_to_string(
            execution
                .run_layout
                .intermediate_artifacts_dir
                .join(WINDOWS_ARTIFACT_FILENAME),
        )
        .expect("window artifact should be readable");
        let windows: Vec<helioframe_core::TemporalWindow> =
            serde_json::from_str(&windows_raw).expect("window artifact should parse as json");
        assert!(!windows.is_empty());
        assert_eq!(windows[0].start_frame, 0);
        assert!(!windows[0].anchor_frames.is_empty());

        std::fs::remove_dir_all(temp).expect("temp directory cleanup should succeed");
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

    fn create_fixture_clip(base: &std::path::Path) -> std::path::PathBuf {
        std::fs::create_dir_all(base).expect("temp fixture directory should be creatable");
        let path = base.join("input.mp4");
        let output = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("testsrc=size=320x180:rate=24:duration=1")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("anullsrc=r=48000:cl=stereo")
            .arg("-shortest")
            .arg("-c:v")
            .arg("libx264")
            .arg("-c:a")
            .arg("aac")
            .arg(&path)
            .output()
            .expect("ffmpeg should run");

        assert!(
            output.status.success(),
            "ffmpeg fixture generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        path
    }
}
