use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use helioframe_core::{
    AppConfig, BackendKind, HelioFrameResult, PresetConfig, RunLayout, RunManifest, RunProbeInfo,
    TemporalQcManifest, TemporalQcSummary, TemporalQcWindowStatus, WindowTileManifest,
};
use helioframe_model::{
    BackendRegistry, DetailRefinementPolicy, InferencePlan, RerunPolicy, TemporalQcPolicy,
    WorkerLaunchConfig,
};
use helioframe_video::{
    decode_to_frame_directory, encode_from_frame_directory, probe_input, DecodePlan, EncodePlan,
    VideoProbe,
};

use crate::shots::{detect_shots, DEFAULT_SCDET_THRESHOLD};
use crate::stages::PipelineStage;
use crate::temporal_qc::{evaluate_windows, evaluate_windows_strict, select_rerun_windows};
use crate::tiling::build_window_tile_manifests;
use crate::windows::build_windows_and_batches;

const WINDOWS_ARTIFACT_FILENAME: &str = "windows.json";
const ANCHORS_ARTIFACT_FILENAME: &str = "anchors.json";
const DETAIL_REFINE_ARTIFACT_FILENAME: &str = "detail_refine.json";
const TEMPORAL_QC_ARTIFACT_FILENAME: &str = "temporal_qc.json";

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

    fn build_temporal_qc_manifest(
        report: &crate::temporal_qc::TemporalQcReport,
    ) -> TemporalQcManifest {
        let windows = report
            .windows
            .iter()
            .map(|window| TemporalQcWindowStatus {
                window_index: window.window_index,
                start_frame: window.start_frame,
                end_frame_exclusive: window.end_frame_exclusive,
                flicker_score: window.flicker_score,
                ghosting_score: window.ghosting_score,
                instability_score: window.instability_score,
                unstable: window.unstable,
                rerun_scheduled: report.rerun_window_indices.contains(&window.window_index),
                reasons: window.reasons.clone(),
            })
            .collect::<Vec<_>>();

        TemporalQcManifest {
            summary: TemporalQcSummary {
                total_windows: windows.len(),
                unstable_windows: report.unstable_window_indices.len(),
                rerun_scheduled_windows: report.rerun_window_indices.len(),
                reject_run: report.should_reject_run,
            },
            windows,
        }
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
        let mut temporal_window_tiles: Option<Vec<WindowTileManifest>> = None;

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
                    temporal_window_tiles = Some(tile_manifests.clone());
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
                        backend.kind(),
                    );
                    let worker_result = backend.worker_adapter().run(worker_launch)?;

                    decoded.frames_dir = worker_result.output_frames_dir.clone();
                    decoded.frame_count = worker_result.frame_count;
                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "refine" => {
                    let started = Instant::now();
                    let decoded = decoded_frames.as_mut().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "refine stage cannot run before decode stage".to_string(),
                        )
                    })?;
                    let windows = temporal_windows.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "refine stage cannot run before window stage".to_string(),
                        )
                    })?;

                    let detail_policy = DetailRefinementPolicy::default();
                    let refiner_options = serde_json::json!({
                        "model_path": "models/detail-refiner/detail_refiner_v1.0.0.ts",
                        "model_version": "detail-refiner-v1.0.0",
                        "weights_sha256": "b8d4f2a1e6c73950d2b1e4a8f7c3d6b9e5a2f8c1d7b4e0a3f6c9d2b5e8a1f4c7",
                        "device": "cuda",
                        "refinement_steps": detail_policy.refinement_steps,
                        "refinement_strength": detail_policy.refinement_strength,
                        "hf_energy_threshold": detail_policy.hf_energy_threshold,
                        "min_window_hf_ratio": detail_policy.min_window_hf_ratio,
                        "max_hf_flicker": detail_policy.sparkle_guard.max_hf_flicker,
                        "max_patch_shimmer": detail_policy.sparkle_guard.max_patch_shimmer,
                        "categories": detail_policy.categories.iter()
                            .map(|c| serde_json::to_value(c).unwrap_or_default())
                            .collect::<Vec<_>>(),
                        "patch_size": 128,
                        "precision": if plan.preset.use_half_precision { "fp16" } else { "fp32" },
                    });

                    // Serialize temporal windows so the refiner can process
                    // each window independently and apply per-window sparkle
                    // guard and HF pre-screening.
                    let window_ranges: Vec<serde_json::Value> = windows
                        .iter()
                        .enumerate()
                        .map(|(idx, w)| {
                            serde_json::json!({
                                "window_index": idx,
                                "start_frame": w.start_frame,
                                "end_frame_exclusive": w.end_frame_exclusive,
                            })
                        })
                        .collect();

                    let refine_worker_io_dir = run_layout
                        .intermediate_artifacts_dir
                        .join("worker_refine");
                    fs::create_dir_all(&refine_worker_io_dir).map_err(|err| {
                        helioframe_core::HelioFrameError::Config(format!(
                            "failed to create refine worker I/O directory: {err}"
                        ))
                    })?;
                    let refine_output_frames_dir = refine_worker_io_dir.join("frames");
                    let refine_input_manifest_path =
                        refine_worker_io_dir.join("refine-input.json");
                    let refine_output_manifest_path =
                        refine_worker_io_dir.join("refine-output.json");

                    let refine_frames: Vec<serde_json::Value> = (0..decoded.frame_count)
                        .map(|index| {
                            serde_json::json!({
                                "index": index,
                                "file_name": format!("frame_{index:010}.png"),
                            })
                        })
                        .collect();

                    let refine_manifest = serde_json::json!({
                        "schema_version": "1.0.0",
                        "run_id": manifest.run_id,
                        "clip_id": "detail-refine-job",
                        "backend": "detail-refiner",
                        "backend_options": refiner_options,
                        "windows": window_ranges,
                        "input_frames_dir": decoded.frames_dir.to_string_lossy(),
                        "output_frames_dir": refine_output_frames_dir.to_string_lossy(),
                        "output_manifest_path": refine_output_manifest_path.to_string_lossy(),
                        "frames": refine_frames,
                    });

                    let payload = serde_json::to_string_pretty(&refine_manifest).map_err(
                        |err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "failed to serialize refine worker input manifest: {err}"
                            ))
                        },
                    )?;
                    fs::write(&refine_input_manifest_path, payload).map_err(|err| {
                        helioframe_core::HelioFrameError::Config(format!(
                            "failed to write refine worker input manifest: {err}"
                        ))
                    })?;

                    let mut child = std::process::Command::new(helioframe_model::python_exe())
                        .arg("workers/python/worker.py")
                        .arg(&refine_input_manifest_path)
                        .spawn()
                        .map_err(|err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "failed to launch detail refiner worker: {err}"
                            ))
                        })?;

                    let timeout = std::time::Duration::from_secs(300);
                    let child_started = Instant::now();
                    loop {
                        if let Some(status) = child.try_wait().map_err(|err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "failed to poll detail refiner worker: {err}"
                            ))
                        })? {
                            if !status.success() {
                                return Err(helioframe_core::HelioFrameError::Config(
                                    format!("detail refiner worker exited with status {status}"),
                                ));
                            }
                            break;
                        }
                        if child_started.elapsed() >= timeout {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Err(helioframe_core::HelioFrameError::Config(
                                "detail refiner worker timed out".to_string(),
                            ));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }

                    let raw_output =
                        fs::read_to_string(&refine_output_manifest_path).map_err(|err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "detail refiner worker did not produce output manifest: {err}"
                            ))
                        })?;

                    let output_parsed: serde_json::Value =
                        serde_json::from_str(&raw_output).map_err(|err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "failed to parse detail refiner output manifest: {err}"
                            ))
                        })?;

                    if output_parsed.get("status").and_then(|v| v.as_str()) != Some("ok") {
                        return Err(helioframe_core::HelioFrameError::Config(format!(
                            "detail refiner worker failed with status `{}`",
                            output_parsed
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown"),
                        )));
                    }

                    let refine_frame_count = output_parsed
                        .get("frame_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as usize;

                    if refine_frame_count != decoded.frame_count {
                        return Err(helioframe_core::HelioFrameError::Config(format!(
                            "detail refiner frame count mismatch: expected {}, got {}",
                            decoded.frame_count, refine_frame_count,
                        )));
                    }

                    // Update decoded frames to point at refined output.
                    decoded.frames_dir = refine_output_frames_dir;

                    // Write refine stage artifact with backend metadata.
                    Self::write_stage_artifact(
                        &run_layout,
                        DETAIL_REFINE_ARTIFACT_FILENAME,
                        &output_parsed,
                        "detail refinement",
                    )?;

                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "temporal-qc" => {
                    let started = Instant::now();
                    let windows = temporal_windows.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "temporal-qc stage cannot run before window stage".to_string(),
                        )
                    })?;
                    let tiles = temporal_window_tiles.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "temporal-qc stage cannot run before tile stage".to_string(),
                        )
                    })?;

                    let qc_policy = TemporalQcPolicy {
                        enabled: plan.preset.enable_temporal_consistency_checks,
                        reject_if_unstable: plan.preset.reject_on_temporal_regression,
                        ..TemporalQcPolicy::default()
                    };
                    let use_strict_gate = matches!(
                        config.backend,
                        BackendKind::StcditStudio
                    );
                    let mut qc_report = if use_strict_gate {
                        evaluate_windows_strict(windows, tiles, &qc_policy)
                    } else {
                        evaluate_windows(windows, tiles, &qc_policy)
                    };
                    let max_attempts = match qc_policy.rerun_policy {
                        Some(RerunPolicy::FailedWindows { max_attempts }) => max_attempts,
                        _ => 0,
                    };
                    for _ in 0..max_attempts {
                        let rerun_window_indices = select_rerun_windows(
                            &qc_report.unstable_window_indices,
                            qc_policy.rerun_policy.as_ref(),
                        );
                        if rerun_window_indices.is_empty() {
                            break;
                        }

                        let decoded = decoded_frames.as_mut().ok_or_else(|| {
                            helioframe_core::HelioFrameError::Config(
                                "temporal-qc rerun requires decoded frames".to_string(),
                            )
                        })?;
                        let failed_tile_jobs = tiles
                            .iter()
                            .filter(|tile| rerun_window_indices.contains(&tile.window_index))
                            .cloned()
                            .collect::<Vec<_>>();

                        let worker_launch = WorkerLaunchConfig::new(
                            &run_layout,
                            &manifest,
                            &decoded.frames_dir,
                            decoded.frame_count,
                            &failed_tile_jobs,
                            backend.kind(),
                        );
                        let worker_result = backend.worker_adapter().run(worker_launch)?;
                        decoded.frames_dir = worker_result.output_frames_dir;
                        decoded.frame_count = worker_result.frame_count;

                        let next_report = evaluate_windows(windows, tiles, &qc_policy);
                        if next_report.unstable_window_indices == qc_report.unstable_window_indices
                        {
                            qc_report = next_report;
                            break;
                        }
                        qc_report = next_report;
                    }

                    Self::write_stage_artifact(
                        &run_layout,
                        TEMPORAL_QC_ARTIFACT_FILENAME,
                        &qc_report,
                        "temporal qc",
                    )?;

                    manifest.set_temporal_qc(Self::build_temporal_qc_manifest(&qc_report));

                    if qc_report.should_reject_run {
                        return Err(helioframe_core::HelioFrameError::Config(format!(
                            "temporal QC rejected run: {} unstable windows detected",
                            qc_report.unstable_window_indices.len()
                        )));
                    }

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
            manifest.mark_stage_completed(stage.name);
            run_layout.write_manifest(&manifest)?;
        }

        Ok(RunExecution { plan, run_layout })
    }

    pub fn resume(
        run_dir: impl AsRef<Path>,
        config: &AppConfig,
        preset: PresetConfig,
    ) -> HelioFrameResult<RunExecution> {
        let run_layout = RunLayout::from_existing(run_dir)?;
        let mut manifest = run_layout.load_manifest()?;
        let plan = Self::plan(config, preset)?;
        let backend = BackendRegistry::resolve(config.backend);

        let mut decoded_frames = None;
        let mut shot_detection = None;
        let mut temporal_windows = None;
        let mut temporal_window_tiles: Option<Vec<WindowTileManifest>> = None;

        // Rebuild in-memory state from artifacts of completed stages.
        Self::recover_state(
            &run_layout,
            &manifest,
            &plan,
            &mut decoded_frames,
            &mut shot_detection,
            &mut temporal_windows,
            &mut temporal_window_tiles,
        )?;

        for stage in &plan.stages {
            if manifest.is_stage_completed(stage.name) {
                continue;
            }

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
                    temporal_window_tiles = Some(tile_manifests.clone());
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

                    // Filter out already-completed windows for resumable restore.
                    let pending_tiles: Vec<WindowTileManifest> = manifest
                        .window_tiles
                        .iter()
                        .filter(|t| !manifest.is_window_completed(t.window_index))
                        .cloned()
                        .collect();

                    if pending_tiles.is_empty() {
                        manifest.append_stage_timing(stage.name, started.elapsed());
                    } else {
                        let worker_launch = WorkerLaunchConfig::new(
                            &run_layout,
                            &manifest,
                            &decoded.frames_dir,
                            decoded.frame_count,
                            &pending_tiles,
                            backend.kind(),
                        );
                        let worker_result = backend.worker_adapter().run(worker_launch)?;

                        // Mark each restored window as completed.
                        for tile in &pending_tiles {
                            manifest.mark_window_completed(tile.window_index);
                        }

                        decoded.frames_dir = worker_result.output_frames_dir.clone();
                        decoded.frame_count = worker_result.frame_count;
                        manifest.append_stage_timing(stage.name, started.elapsed());
                    }
                }
                "refine" => {
                    let started = Instant::now();
                    let decoded = decoded_frames.as_mut().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "refine stage cannot run before decode stage".to_string(),
                        )
                    })?;
                    let windows = temporal_windows.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "refine stage cannot run before window stage".to_string(),
                        )
                    })?;

                    let detail_policy = DetailRefinementPolicy::default();
                    let refiner_options = serde_json::json!({
                        "model_path": "models/detail-refiner/detail_refiner_v1.0.0.ts",
                        "model_version": "detail-refiner-v1.0.0",
                        "weights_sha256": "b8d4f2a1e6c73950d2b1e4a8f7c3d6b9e5a2f8c1d7b4e0a3f6c9d2b5e8a1f4c7",
                        "device": "cuda",
                        "refinement_steps": detail_policy.refinement_steps,
                        "refinement_strength": detail_policy.refinement_strength,
                        "hf_energy_threshold": detail_policy.hf_energy_threshold,
                        "min_window_hf_ratio": detail_policy.min_window_hf_ratio,
                        "max_hf_flicker": detail_policy.sparkle_guard.max_hf_flicker,
                        "max_patch_shimmer": detail_policy.sparkle_guard.max_patch_shimmer,
                        "categories": detail_policy.categories.iter()
                            .map(|c| serde_json::to_value(c).unwrap_or_default())
                            .collect::<Vec<_>>(),
                        "patch_size": 128,
                        "precision": if plan.preset.use_half_precision { "fp16" } else { "fp32" },
                    });

                    let window_ranges: Vec<serde_json::Value> = windows
                        .iter()
                        .enumerate()
                        .map(|(idx, w)| {
                            serde_json::json!({
                                "window_index": idx,
                                "start_frame": w.start_frame,
                                "end_frame_exclusive": w.end_frame_exclusive,
                            })
                        })
                        .collect();

                    let refine_worker_io_dir = run_layout
                        .intermediate_artifacts_dir
                        .join("worker_refine");
                    fs::create_dir_all(&refine_worker_io_dir).map_err(|err| {
                        helioframe_core::HelioFrameError::Config(format!(
                            "failed to create refine worker I/O directory: {err}"
                        ))
                    })?;
                    let refine_output_frames_dir = refine_worker_io_dir.join("frames");
                    let refine_input_manifest_path =
                        refine_worker_io_dir.join("refine-input.json");
                    let refine_output_manifest_path =
                        refine_worker_io_dir.join("refine-output.json");

                    let refine_frames: Vec<serde_json::Value> = (0..decoded.frame_count)
                        .map(|index| {
                            serde_json::json!({
                                "index": index,
                                "file_name": format!("frame_{index:010}.png"),
                            })
                        })
                        .collect();

                    let refine_manifest = serde_json::json!({
                        "schema_version": "1.0.0",
                        "run_id": manifest.run_id,
                        "clip_id": "detail-refine-job",
                        "backend": "detail-refiner",
                        "backend_options": refiner_options,
                        "windows": window_ranges,
                        "input_frames_dir": decoded.frames_dir.to_string_lossy(),
                        "output_frames_dir": refine_output_frames_dir.to_string_lossy(),
                        "output_manifest_path": refine_output_manifest_path.to_string_lossy(),
                        "frames": refine_frames,
                    });

                    let payload = serde_json::to_string_pretty(&refine_manifest).map_err(
                        |err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "failed to serialize refine worker input manifest: {err}"
                            ))
                        },
                    )?;
                    fs::write(&refine_input_manifest_path, payload).map_err(|err| {
                        helioframe_core::HelioFrameError::Config(format!(
                            "failed to write refine worker input manifest: {err}"
                        ))
                    })?;

                    let mut child = std::process::Command::new(helioframe_model::python_exe())
                        .arg("workers/python/worker.py")
                        .arg(&refine_input_manifest_path)
                        .spawn()
                        .map_err(|err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "failed to launch detail refiner worker: {err}"
                            ))
                        })?;

                    let timeout = std::time::Duration::from_secs(300);
                    let child_started = Instant::now();
                    loop {
                        if let Some(status) = child.try_wait().map_err(|err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "failed to poll detail refiner worker: {err}"
                            ))
                        })? {
                            if !status.success() {
                                return Err(helioframe_core::HelioFrameError::Config(
                                    format!("detail refiner worker exited with status {status}"),
                                ));
                            }
                            break;
                        }
                        if child_started.elapsed() >= timeout {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Err(helioframe_core::HelioFrameError::Config(
                                "detail refiner worker timed out".to_string(),
                            ));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }

                    let raw_output =
                        fs::read_to_string(&refine_output_manifest_path).map_err(|err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "detail refiner worker did not produce output manifest: {err}"
                            ))
                        })?;

                    let output_parsed: serde_json::Value =
                        serde_json::from_str(&raw_output).map_err(|err| {
                            helioframe_core::HelioFrameError::Config(format!(
                                "failed to parse detail refiner output manifest: {err}"
                            ))
                        })?;

                    if output_parsed.get("status").and_then(|v| v.as_str()) != Some("ok") {
                        return Err(helioframe_core::HelioFrameError::Config(format!(
                            "detail refiner worker failed with status `{}`",
                            output_parsed
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown"),
                        )));
                    }

                    let refine_frame_count = output_parsed
                        .get("frame_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as usize;

                    if refine_frame_count != decoded.frame_count {
                        return Err(helioframe_core::HelioFrameError::Config(format!(
                            "detail refiner frame count mismatch: expected {}, got {}",
                            decoded.frame_count, refine_frame_count,
                        )));
                    }

                    decoded.frames_dir = refine_output_frames_dir;

                    Self::write_stage_artifact(
                        &run_layout,
                        DETAIL_REFINE_ARTIFACT_FILENAME,
                        &output_parsed,
                        "detail refinement",
                    )?;

                    manifest.append_stage_timing(stage.name, started.elapsed());
                }
                "temporal-qc" => {
                    let started = Instant::now();
                    let windows = temporal_windows.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "temporal-qc stage cannot run before window stage".to_string(),
                        )
                    })?;
                    let tiles = temporal_window_tiles.as_ref().ok_or_else(|| {
                        helioframe_core::HelioFrameError::Config(
                            "temporal-qc stage cannot run before tile stage".to_string(),
                        )
                    })?;

                    let qc_policy = TemporalQcPolicy {
                        enabled: plan.preset.enable_temporal_consistency_checks,
                        reject_if_unstable: plan.preset.reject_on_temporal_regression,
                        ..TemporalQcPolicy::default()
                    };
                    let use_strict_gate = matches!(
                        config.backend,
                        BackendKind::StcditStudio
                    );
                    let mut qc_report = if use_strict_gate {
                        evaluate_windows_strict(windows, tiles, &qc_policy)
                    } else {
                        evaluate_windows(windows, tiles, &qc_policy)
                    };
                    let max_attempts = match qc_policy.rerun_policy {
                        Some(RerunPolicy::FailedWindows { max_attempts }) => max_attempts,
                        _ => 0,
                    };
                    for _ in 0..max_attempts {
                        let rerun_window_indices = select_rerun_windows(
                            &qc_report.unstable_window_indices,
                            qc_policy.rerun_policy.as_ref(),
                        );
                        if rerun_window_indices.is_empty() {
                            break;
                        }
                        let decoded = decoded_frames.as_mut().ok_or_else(|| {
                            helioframe_core::HelioFrameError::Config(
                                "temporal-qc rerun requires decoded frames".to_string(),
                            )
                        })?;
                        let failed_tile_jobs = tiles
                            .iter()
                            .filter(|tile| rerun_window_indices.contains(&tile.window_index))
                            .cloned()
                            .collect::<Vec<_>>();
                        let worker_launch = WorkerLaunchConfig::new(
                            &run_layout,
                            &manifest,
                            &decoded.frames_dir,
                            decoded.frame_count,
                            &failed_tile_jobs,
                            backend.kind(),
                        );
                        let worker_result = backend.worker_adapter().run(worker_launch)?;
                        decoded.frames_dir = worker_result.output_frames_dir;
                        decoded.frame_count = worker_result.frame_count;
                        let next_report = evaluate_windows(windows, tiles, &qc_policy);
                        if next_report.unstable_window_indices == qc_report.unstable_window_indices
                        {
                            qc_report = next_report;
                            break;
                        }
                        qc_report = next_report;
                    }

                    Self::write_stage_artifact(
                        &run_layout,
                        TEMPORAL_QC_ARTIFACT_FILENAME,
                        &qc_report,
                        "temporal qc",
                    )?;

                    manifest.set_temporal_qc(Self::build_temporal_qc_manifest(&qc_report));

                    if qc_report.should_reject_run {
                        return Err(helioframe_core::HelioFrameError::Config(format!(
                            "temporal QC rejected run: {} unstable windows detected",
                            qc_report.unstable_window_indices.len()
                        )));
                    }

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
            manifest.mark_stage_completed(stage.name);
            run_layout.write_manifest(&manifest)?;
        }

        Ok(RunExecution { plan, run_layout })
    }

    /// Rebuild in-memory pipeline state from artifacts saved by completed stages.
    fn recover_state(
        run_layout: &RunLayout,
        manifest: &RunManifest,
        plan: &ExecutionPlan,
        decoded_frames: &mut Option<helioframe_video::DecodedFrames>,
        shot_detection: &mut Option<crate::stages::ShotDetectionArtifact>,
        temporal_windows: &mut Option<Vec<helioframe_core::TemporalWindow>>,
        temporal_window_tiles: &mut Option<Vec<WindowTileManifest>>,
    ) -> HelioFrameResult<()> {
        // Recover decoded frames if decode completed.
        if manifest.is_stage_completed("decode") {
            let decode_dir = run_layout.intermediate_artifacts_dir.join("decoded");
            if decode_dir.exists() {
                let frame_count = fs::read_dir(&decode_dir)
                    .map(|entries| {
                        entries
                            .filter_map(|e| e.ok())
                            .filter(|e| {
                                e.path()
                                    .extension()
                                    .map(|ext| ext == "png")
                                    .unwrap_or(false)
                            })
                            .count()
                    })
                    .unwrap_or(0);

                // Check for refined output first (restore may have updated frames_dir).
                let restore_output_dir = run_layout.intermediate_artifacts_dir.join("worker_output");
                let refine_output_dir = run_layout
                    .intermediate_artifacts_dir
                    .join("worker_refine")
                    .join("frames");

                let (frames_dir, count) = if manifest.is_stage_completed("refine")
                    && refine_output_dir.exists()
                {
                    let refine_count = fs::read_dir(&refine_output_dir)
                        .map(|entries| {
                            entries
                                .filter_map(|e| e.ok())
                                .filter(|e| {
                                    e.path()
                                        .extension()
                                        .map(|ext| ext == "png")
                                        .unwrap_or(false)
                                })
                                .count()
                        })
                        .unwrap_or(frame_count);
                    (refine_output_dir, refine_count)
                } else if manifest.is_stage_completed("restore") && restore_output_dir.exists() {
                    let restore_count = fs::read_dir(&restore_output_dir)
                        .map(|entries| {
                            entries
                                .filter_map(|e| e.ok())
                                .filter(|e| {
                                    e.path()
                                        .extension()
                                        .map(|ext| ext == "png")
                                        .unwrap_or(false)
                                })
                                .count()
                        })
                        .unwrap_or(frame_count);
                    (restore_output_dir, restore_count)
                } else {
                    (decode_dir, frame_count)
                };

                // Scan for extracted audio alongside decoded frames.
                let decode_base = run_layout.intermediate_artifacts_dir.join("decoded");
                let audio_path = decode_base.join("audio.aac");
                *decoded_frames = Some(helioframe_video::DecodedFrames {
                    frames_dir,
                    frame_pattern: "frame_%010d.png".to_string(),
                    timestamps_path: decode_base.join("timestamps.txt"),
                    frame_count: count,
                    fps: plan.probe.fps,
                    duration_seconds: plan.probe.duration_seconds,
                    audio_path: if audio_path.exists() {
                        Some(audio_path)
                    } else {
                        None
                    },
                });
            }
        }

        // Recover shot detection from artifact.
        if manifest.is_stage_completed("shots") {
            let shots_path = run_layout.intermediate_artifacts_dir.join("shots.json");
            if shots_path.exists() {
                let raw = fs::read_to_string(&shots_path).map_err(|err| {
                    helioframe_core::HelioFrameError::Config(format!(
                        "failed to read shots artifact: {err}"
                    ))
                })?;
                let detections: crate::stages::ShotDetectionArtifact =
                    serde_json::from_str(&raw).map_err(|err| {
                        helioframe_core::HelioFrameError::Config(format!(
                            "failed to parse shots artifact: {err}"
                        ))
                    })?;
                *shot_detection = Some(detections);
            }
        }

        // Recover temporal windows from manifest.
        if manifest.is_stage_completed("window") && !manifest.windows.is_empty() {
            *temporal_windows = Some(manifest.windows.clone());
        }

        // Recover tile manifests from manifest.
        if manifest.is_stage_completed("tile") && !manifest.window_tiles.is_empty() {
            *temporal_window_tiles = Some(manifest.window_tiles.clone());
        }

        Ok(())
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
        assert!(manifest.temporal_qc.is_some());
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
        assert!(execution
            .run_layout
            .intermediate_artifacts_dir
            .join(TEMPORAL_QC_ARTIFACT_FILENAME)
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
