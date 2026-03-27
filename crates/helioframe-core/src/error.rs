use thiserror::Error;

pub type HelioFrameResult<T> = Result<T, HelioFrameError>;

#[derive(Debug, Error)]
pub enum HelioFrameError {
    #[error("unsupported video container: {0}")]
    UnsupportedContainer(String),
    #[error("invalid resolution: {0}x{1}")]
    InvalidResolution(u32, u32),
    #[error("unsupported target resolution: {0}x{1}")]
    UnsupportedTargetResolution(u32, u32),
    #[error("invalid tile configuration: tile_size ({tile_size}) must be greater than overlap ({overlap})")]
    InvalidTileConfiguration { tile_size: usize, overlap: usize },
    #[error("preset `{preset}` does not allow backend `{backend}`")]
    PresetBackendMismatch { preset: String, backend: String },
    #[error("backend `{backend}` is not eligible for strict 4K policy")]
    BackendNotStrict4k { backend: String },
    #[error("preset file name `{actual}` does not match expected preset `{expected}`")]
    PresetNameMismatch { expected: String, actual: String },
    #[error("configuration error: {0}")]
    Config(String),
}
