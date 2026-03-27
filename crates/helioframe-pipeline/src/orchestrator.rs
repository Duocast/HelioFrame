use std::path::Path;

use helioframe_core::{AppConfig, HelioFrameResult, PresetConfig};
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

pub struct PipelineOrchestrator;

impl PipelineOrchestrator {
    pub fn plan(config: &AppConfig, preset: PresetConfig) -> HelioFrameResult<ExecutionPlan> {
        config.validate()?;
        let probe = probe_input(Path::new(&config.input))?;
        let backend = BackendRegistry::resolve(config.backend);
        let inference = backend.build_plan(config.target_resolution);

        let mut stages = vec![
            PipelineStage {
                name: "probe",
                description: "Inspect container, stream metadata, codec compatibility, and source resolution.",
            },
            PipelineStage {
                name: "decode",
                description: "Decode video frames and audio through FFmpeg-backed I/O.",
            },
            PipelineStage {
                name: "normalize",
                description: "Normalize pixel format, transfer characteristics, colorspace, and tensor layout.",
            },
            PipelineStage {
                name: "shots",
                description: "Detect scene boundaries so motion and guidance state do not leak across cuts.",
            },
            PipelineStage {
                name: "anchors",
                description: "Select anchor frames and guidance frames for structure-sensitive restoration.",
            },
            PipelineStage {
                name: "window",
                description: "Build temporal windows sized for high-quality restoration rather than maximum throughput.",
            },
            PipelineStage {
                name: "tile",
                description: "Schedule overlap-heavy spatial patches sized for 4K reconstruction and seam suppression.",
            },
        ];

        stages.push(PipelineStage {
            name: "restore",
            description: "Run the primary restoration backend over each temporal window and patch batch.",
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
                description: "Blend overlaps, reconstruct full frames, and remove tile boundary artifacts.",
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
}
