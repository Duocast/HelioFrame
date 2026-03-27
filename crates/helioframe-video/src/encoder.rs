use helioframe_core::Resolution;

#[derive(Debug, Clone)]
pub struct EncodePlan {
    pub output_resolution: Resolution,
    pub preserve_audio: bool,
    pub container_hint: &'static str,
}
