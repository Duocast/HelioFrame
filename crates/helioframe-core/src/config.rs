use crate::{
    BackendKind, HelioFrameError, HelioFrameResult, Resolution, UpscalePreset, VideoContainer,
};
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
        VideoContainer::from_path(Path::new(&self.input))?;
        VideoContainer::from_path(Path::new(&self.output))?;
        self.target_resolution.validate()?;

        if !self.backend.supports_strict_4k() {
            return Err(HelioFrameError::BackendNotStrict4k {
                backend: self.backend.to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetConfig {
    pub name: String,
    pub default_backend: BackendKind,
    pub allowed_backends: Vec<BackendKind>,
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
        let config: Self = toml::from_str(&raw)
            .map_err(|err| HelioFrameError::Config(format!("failed to parse preset: {err}")))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> HelioFrameResult<()> {
        if self.tile_size <= self.overlap {
            return Err(HelioFrameError::InvalidTileConfiguration {
                tile_size: self.tile_size,
                overlap: self.overlap,
            });
        }

        if !self.allowed_backends.contains(&self.default_backend) {
            return Err(HelioFrameError::Config(format!(
                "default backend `{}` is not in allowed_backends for preset `{}`",
                self.default_backend, self.name
            )));
        }

        if !self.default_backend.supports_strict_4k() {
            return Err(HelioFrameError::BackendNotStrict4k {
                backend: self.default_backend.to_string(),
            });
        }

        for backend in &self.allowed_backends {
            if !backend.supports_strict_4k() {
                return Err(HelioFrameError::BackendNotStrict4k {
                    backend: backend.to_string(),
                });
            }
        }

        Ok(())
    }

    pub fn validate_selection(
        &self,
        selected_preset: UpscalePreset,
        selected_backend: BackendKind,
    ) -> HelioFrameResult<()> {
        let expected_preset_name = selected_preset.to_string();
        if self.name != expected_preset_name {
            return Err(HelioFrameError::PresetNameMismatch {
                expected: expected_preset_name,
                actual: self.name.clone(),
            });
        }

        if !self.allowed_backends.contains(&selected_backend) {
            return Err(HelioFrameError::PresetBackendMismatch {
                preset: self.name.clone(),
                backend: selected_backend.to_string(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn app_config_validation_rejects_unsupported_output_container() {
        let config = AppConfig {
            input: "input.mp4".into(),
            output: "output.unsupported".into(),
            backend: BackendKind::StcditStudio,
            preset: UpscalePreset::Studio,
            target_resolution: Resolution::UHD_4K,
        };

        let err = config
            .validate()
            .expect_err("expected container validation to fail");
        assert!(matches!(err, HelioFrameError::UnsupportedContainer(_)));
    }

    #[test]
    fn app_config_validation_accepts_classical_baseline_backend() {
        let config = AppConfig {
            input: "input.mp4".into(),
            output: "output.mp4".into(),
            backend: BackendKind::ClassicalBaseline,
            preset: UpscalePreset::Balanced,
            target_resolution: Resolution::UHD_4K,
        };

        config
            .validate()
            .expect("classical baseline should now be accepted for strict 4K runs");
    }

    #[test]
    fn preset_validation_rejects_non_sensical_tile_configuration() {
        let mut preset = sample_preset();
        preset.tile_size = 64;
        preset.overlap = 64;

        let err = preset
            .validate()
            .expect_err("expected tile validation to fail");
        assert!(matches!(
            err,
            HelioFrameError::InvalidTileConfiguration {
                tile_size: 64,
                overlap: 64
            }
        ));
    }

    #[test]
    fn preset_selection_validation_rejects_disallowed_backend() {
        let preset = sample_preset();

        let err = preset
            .validate_selection(UpscalePreset::Studio, BackendKind::FastPreview)
            .expect_err("expected backend mismatch validation to fail");
        assert!(matches!(err, HelioFrameError::PresetBackendMismatch { .. }));
    }

    #[test]
    fn preset_validation_accepts_classical_baseline_allowed_backends() {
        let mut preset = sample_preset();
        preset.allowed_backends.push(BackendKind::ClassicalBaseline);

        preset
            .validate()
            .expect("classical baseline should be accepted in allowed backends");
    }
}
