use helioframe_core::{BackendKind, Resolution};

use crate::{backend::BackendCapabilities, plan::InferencePlan};

#[derive(Debug, Clone, Copy)]
pub struct BackendExecutionProfile {
    pub deterministic_output: bool,
    pub enable_mild_denoise: bool,
    pub resize_filter: &'static str,
    pub sharpen_amount: Option<f32>,
}

impl Default for BackendExecutionProfile {
    fn default() -> Self {
        Self {
            deterministic_output: false,
            enable_mild_denoise: false,
            resize_filter: "lanczos",
            sharpen_amount: None,
        }
    }
}

pub trait InferenceBackend {
    fn kind(&self) -> BackendKind;
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> BackendCapabilities;
    fn build_plan(&self, target_resolution: Resolution) -> InferencePlan;
    fn execution_profile(&self) -> BackendExecutionProfile {
        BackendExecutionProfile::default()
    }
}
