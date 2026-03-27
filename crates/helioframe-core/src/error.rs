use thiserror::Error;

pub type HelioFrameResult<T> = Result<T, HelioFrameError>;

#[derive(Debug, Error)]
pub enum HelioFrameError {
    #[error("unsupported video container: {0}")]
    UnsupportedContainer(String),
    #[error("invalid resolution: {0}x{1}")]
    InvalidResolution(u32, u32),
    #[error("configuration error: {0}")]
    Config(String),
}
