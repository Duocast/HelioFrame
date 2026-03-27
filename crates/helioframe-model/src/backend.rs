use helioframe_core::{BackendKind, Resolution};

use crate::{
    plan::{ExecutionHints, InferencePlan},
    traits::{BackendExecutionProfile, InferenceBackend},
};

#[derive(Debug, Clone, Copy)]
pub struct BackendCapabilities {
    pub real_world_restoration: bool,
    pub patch_wise_4k: bool,
    pub structural_guidance: bool,
    pub detail_refiner: bool,
    pub temporal_qc_gate: bool,
    pub teacher_guided: bool,
    pub multi_step_diffusion: bool,
    pub custom_cuda_recommended: bool,
}

pub struct BackendRegistry;

impl BackendRegistry {
    pub fn resolve(kind: BackendKind) -> Box<dyn InferenceBackend> {
        match kind {
            BackendKind::ClassicalBaseline => Box::new(ClassicalBaseline),
            BackendKind::FastPreview => Box::new(FastPreview),
            BackendKind::SeedvrTeacher => Box::new(SeedvrTeacher),
            BackendKind::StcditStudio => Box::new(StcditStudio),
            BackendKind::HelioFrameMaster => Box::new(HelioFrameMaster),
        }
    }
}

struct ClassicalBaseline;
struct FastPreview;
struct SeedvrTeacher;
struct StcditStudio;
struct HelioFrameMaster;

impl InferenceBackend for ClassicalBaseline {
    fn kind(&self) -> BackendKind {
        BackendKind::ClassicalBaseline
    }

    fn name(&self) -> &'static str {
        "Classical baseline"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            real_world_restoration: true,
            patch_wise_4k: false,
            structural_guidance: false,
            detail_refiner: false,
            temporal_qc_gate: true,
            teacher_guided: false,
            multi_step_diffusion: false,
            custom_cuda_recommended: false,
        }
    }

    fn build_plan(&self, target_resolution: Resolution) -> InferencePlan {
        InferencePlan {
            backend: self.kind(),
            target_resolution,
            summary: "Deterministic baseline for robust integration, regression comparison, and conservative restoration output.".into(),
            hints: ExecutionHints {
                patch_wise_4k: false,
                multi_step_diffusion: false,
                structural_guidance: false,
                detail_refiner: false,
                temporal_qc_gate: true,
                teacher_guided: false,
                custom_kernels_recommended: false,
            },
        }
    }

    fn execution_profile(&self) -> BackendExecutionProfile {
        BackendExecutionProfile {
            deterministic_output: true,
            enable_mild_denoise: false,
            resize_filter: "lanczos",
            sharpen_amount: None,
        }
    }
}

impl InferenceBackend for FastPreview {
    fn kind(&self) -> BackendKind {
        BackendKind::FastPreview
    }

    fn name(&self) -> &'static str {
        "Fast preview"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            real_world_restoration: true,
            patch_wise_4k: true,
            structural_guidance: false,
            detail_refiner: false,
            temporal_qc_gate: false,
            teacher_guided: false,
            multi_step_diffusion: false,
            custom_cuda_recommended: true,
        }
    }

    fn build_plan(&self, target_resolution: Resolution) -> InferencePlan {
        InferencePlan {
            backend: self.kind(),
            target_resolution,
            summary: "Distilled preview backend intended for quick turnaround and approximate visual review before committing to a studio render.".into(),
            hints: ExecutionHints {
                patch_wise_4k: target_resolution.width >= 3840,
                multi_step_diffusion: false,
                structural_guidance: false,
                detail_refiner: false,
                temporal_qc_gate: false,
                teacher_guided: false,
                custom_kernels_recommended: true,
            },
        }
    }
}

impl InferenceBackend for SeedvrTeacher {
    fn kind(&self) -> BackendKind {
        BackendKind::SeedvrTeacher
    }

    fn name(&self) -> &'static str {
        "SeedVR teacher"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            real_world_restoration: true,
            patch_wise_4k: true,
            structural_guidance: true,
            detail_refiner: true,
            temporal_qc_gate: true,
            teacher_guided: true,
            multi_step_diffusion: true,
            custom_cuda_recommended: true,
        }
    }

    fn build_plan(&self, target_resolution: Resolution) -> InferencePlan {
        InferencePlan {
            backend: self.kind(),
            target_resolution,
            summary: "Heavy reference-grade restoration plan built for offline quality, teacher-style guidance, and aggressive detail recovery.".into(),
            hints: ExecutionHints {
                patch_wise_4k: true,
                multi_step_diffusion: true,
                structural_guidance: true,
                detail_refiner: true,
                temporal_qc_gate: true,
                teacher_guided: true,
                custom_kernels_recommended: true,
            },
        }
    }
}

impl InferenceBackend for StcditStudio {
    fn kind(&self) -> BackendKind {
        BackendKind::StcditStudio
    }

    fn name(&self) -> &'static str {
        "STCDiT studio"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            real_world_restoration: true,
            patch_wise_4k: true,
            structural_guidance: true,
            detail_refiner: true,
            temporal_qc_gate: true,
            teacher_guided: false,
            multi_step_diffusion: true,
            custom_cuda_recommended: true,
        }
    }

    fn build_plan(&self, target_resolution: Resolution) -> InferencePlan {
        InferencePlan {
            backend: self.kind(),
            target_resolution,
            summary: "Primary studio backend using multi-step diffusion, structural guidance, patch-wise 4K scheduling, and strict temporal quality control.".into(),
            hints: ExecutionHints {
                patch_wise_4k: true,
                multi_step_diffusion: true,
                structural_guidance: true,
                detail_refiner: true,
                temporal_qc_gate: true,
                teacher_guided: false,
                custom_kernels_recommended: true,
            },
        }
    }
}

impl InferenceBackend for HelioFrameMaster {
    fn kind(&self) -> BackendKind {
        BackendKind::HelioFrameMaster
    }

    fn name(&self) -> &'static str {
        "HelioFrame master"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            real_world_restoration: true,
            patch_wise_4k: true,
            structural_guidance: true,
            detail_refiner: true,
            temporal_qc_gate: true,
            teacher_guided: true,
            multi_step_diffusion: true,
            custom_cuda_recommended: true,
        }
    }

    fn build_plan(&self, target_resolution: Resolution) -> InferencePlan {
        InferencePlan {
            backend: self.kind(),
            target_resolution,
            summary: "Experimental flagship plan combining teacher-guided restoration, structure-aware diffusion, dedicated detail refinement, and rejection-driven temporal QC.".into(),
            hints: ExecutionHints {
                patch_wise_4k: true,
                multi_step_diffusion: true,
                structural_guidance: true,
                detail_refiner: true,
                temporal_qc_gate: true,
                teacher_guided: true,
                custom_kernels_recommended: true,
            },
        }
    }
}
