use std::path::Path;

use helioframe_core::{HelioFrameResult, Resolution, VideoContainer};

#[derive(Debug, Clone)]
pub struct VideoProbe {
    pub container: VideoContainer,
    pub assumed_resolution: Resolution,
}

pub fn probe_input(path: &Path) -> HelioFrameResult<VideoProbe> {
    let container = VideoContainer::from_path(path)?;
    Ok(VideoProbe {
        container,
        assumed_resolution: Resolution {
            width: 1920,
            height: 1080,
        },
    })
}
