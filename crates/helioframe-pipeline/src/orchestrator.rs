use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use helioframe_core::{
    AppConfig, HelioFrameResult, PresetConfig, RunLayout, RunManifest, RunProbeInfo,
};
use helioframe_model::{BackendRegistry, InferencePlan};
use helioframe_video::{probe_input, DecodePlan, EncodePlan, VideoProbe};

use crate::stages::PipelineStage;

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
    pub fn plan(config: &AppConfig, preset: PresetConfig) -> HelioFrameResult<ExecutionPlan> {
        config.validate()?;
        preset.validate_selection(config.preset, config.backend)?;
        let probe = probe_input(Path::new(&config.input))?;
        let backend = BackendRegistry::resolve(config.backend);
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
                name: "anchors",
                description:
                    "Select anchor frames and guidance frames for structure-sensitive restoration.",
            },
            PipelineStage {
                name: "window",
                description:
                    "Build temporal windows sized for high-quality restoration rather than maximum throughput.",
            },
            PipelineStage {
                name: "tile",
                description:
                    "Schedule overlap-heavy spatial patches sized for 4K reconstruction and seam suppression.",
            },
        ];

        stages.push(PipelineStage {
            name: "restore",
            description:
                "Run the primary restoration backend over each temporal window and patch batch.",
        });

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

        Ok(ExecutionPlan {
            probe,
            preset,
            decode: DecodePlan::default(),
            encode: EncodePlan {
                output_resolution: config.target_resolution,
                preserve_audio: true,
                container_hint: "mp4",
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
        let run_layout = RunLayout::create(base_dir)?;

        let probe = RunProbeInfo {
            container: plan.probe.container.to_string(),
            assumed_resolution: plan.probe.assumed_resolution.to_string(),
        };
        let mut manifest = RunManifest::new(run_layout.run_id.clone(), config, probe);
        run_layout.write_manifest(&manifest)?;

        for stage in &plan.stages {
            let started = Instant::now();
            std::thread::sleep(Duration::from_millis(1));
            manifest.append_stage_timing(stage.name, started.elapsed());
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
