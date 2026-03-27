use crate::{BackendKind, HelioFrameError, HelioFrameResult, Resolution, UpscalePreset};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub input: String,
    pub output: String,
    pub backend: BackendKind,
    pub preset: UpscalePreset,
    pub target_resolution: Resolution,
}

impl AppConfig {
    pub fn validate(&self) -> HelioFrameResult<()> {
        self.target_resolution.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetConfig {
    pub name: String,
    pub default_backend: BackendKind,
    pub temporal_window: usize,
    pub tile_size: usize,
    pub overlap: usize,
    pub diffusion_steps: usize,
    pub use_half_precision: bool,
    pub enable_patchwise_4k: bool,
    pub enable_structural_guidance: bool,
    pub enable_detail_refiner: bool,
    pub enable_temporal_consistency_checks: bool,
    pub reject_on_temporal_regression: bool,
    pub anchor_frame_stride: usize,
    pub notes: String,
}

impl PresetConfig {
    pub fn load_from_file(path: impl AsRef<Path>) -> HelioFrameResult<Self> {
        let raw = fs::read_to_string(path.as_ref())
            .map_err(|err| HelioFrameError::Config(format!("failed to read preset: {err}")))?;
        toml::from_str(&raw)
            .map_err(|err| HelioFrameError::Config(format!("failed to parse preset: {err}")))
    }
}
