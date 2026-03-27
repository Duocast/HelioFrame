use helioframe_core::{BackendKind, Resolution};

use crate::{backend::BackendCapabilities, plan::InferencePlan};

pub trait InferenceBackend {
    fn kind(&self) -> BackendKind;
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> BackendCapabilities;
    fn build_plan(&self, target_resolution: Resolution) -> InferencePlan;
}
