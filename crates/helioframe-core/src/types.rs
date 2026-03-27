use serde::{Deserialize, Serialize};
use std::{fmt, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneBoundary {
    pub frame_index: usize,
    pub timestamp_seconds: f64,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemporalWindow {
    pub start_frame: usize,
    pub end_frame_exclusive: usize,
    pub anchor_frames: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowedClipBatch {
    pub window_index: usize,
    pub start_frame: usize,
    pub end_frame_exclusive: usize,
    pub anchor_frames: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpatialTile {
    pub tile_index: usize,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowTileManifest {
    pub window_index: usize,
    pub start_frame: usize,
    pub end_frame_exclusive: usize,
    pub tiles: Vec<SpatialTile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    pub const UHD_4K: Self = Self {
        width: 3840,
        height: 2160,
    };

    pub fn validate(self) -> Result<Self, crate::error::HelioFrameError> {
        if self.width == 0 || self.height == 0 {
            return Err(crate::error::HelioFrameError::InvalidResolution(
                self.width,
                self.height,
            ));
        }

        if self != Self::UHD_4K {
            return Err(crate::error::HelioFrameError::UnsupportedTargetResolution(
                self.width,
                self.height,
            ));
        }
        Ok(self)
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpscalePreset {
    Preview,
    Balanced,
    Studio,
    Experimental,
}

impl fmt::Display for UpscalePreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Preview => "preview",
            Self::Balanced => "balanced",
            Self::Studio => "studio",
            Self::Experimental => "experimental",
        };
        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    ClassicalBaseline,
    FastPreview,
    SeedvrTeacher,
    StcditStudio,
    HelioFrameMaster,
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ClassicalBaseline => "classical-baseline",
            Self::FastPreview => "fast-preview",
            Self::SeedvrTeacher => "seedvr-teacher",
            Self::StcditStudio => "stcdit-studio",
            Self::HelioFrameMaster => "helioframe-master",
        };
        write!(f, "{value}")
    }
}

impl BackendKind {
    pub fn supports_strict_4k(self) -> bool {
        match self {
            Self::ClassicalBaseline => true,
            Self::FastPreview => true,
            Self::SeedvrTeacher => true,
            Self::StcditStudio => true,
            Self::HelioFrameMaster => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoContainer {
    Mp4,
    Mov,
    Mkv,
    Avi,
    Webm,
    M4v,
}

impl VideoContainer {
    pub fn from_path(path: &Path) -> Result<Self, crate::error::HelioFrameError> {
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        match ext.as_str() {
            "mp4" => Ok(Self::Mp4),
            "mov" => Ok(Self::Mov),
            "mkv" => Ok(Self::Mkv),
            "avi" => Ok(Self::Avi),
            "webm" => Ok(Self::Webm),
            "m4v" => Ok(Self::M4v),
            _ => Err(crate::error::HelioFrameError::UnsupportedContainer(ext)),
        }
    }
}

impl fmt::Display for VideoContainer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Mkv => "mkv",
            Self::Avi => "avi",
            Self::Webm => "webm",
            Self::M4v => "m4v",
        };
        write!(f, "{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::Resolution;
    use crate::HelioFrameError;

    #[test]
    fn resolution_validation_rejects_non_4k_targets() {
        let err = Resolution {
            width: 1920,
            height: 1080,
        }
        .validate()
        .expect_err("expected unsupported target resolution");
        assert!(matches!(
            err,
            HelioFrameError::UnsupportedTargetResolution(1920, 1080)
        ));
    }
}
