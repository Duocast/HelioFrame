#[derive(Debug, Clone)]
pub struct DecodePlan {
    pub use_ffmpeg: bool,
    pub preserve_audio: bool,
}

impl Default for DecodePlan {
    fn default() -> Self {
        Self {
            use_ffmpeg: true,
            preserve_audio: true,
        }
    }
}
