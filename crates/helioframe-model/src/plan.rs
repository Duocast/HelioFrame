use helioframe_core::{BackendKind, Resolution};

#[derive(Debug, Clone)]
pub struct ExecutionHints {
    pub patch_wise_4k: bool,
    pub multi_step_diffusion: bool,
    pub structural_guidance: bool,
    pub detail_refiner: bool,
    pub temporal_qc_gate: bool,
    pub teacher_guided: bool,
    pub custom_kernels_recommended: bool,
    pub temporal_window_inference: bool,
    pub bridge_backend_label: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct InferencePlan {
    pub backend: BackendKind,
    pub target_resolution: Resolution,
    pub summary: String,
    pub hints: ExecutionHints,
}
