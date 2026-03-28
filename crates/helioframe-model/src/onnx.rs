//! ONNX Runtime-backed inference for the fast-preview backend.
//!
//! Runs a single super-resolution ONNX model directly in Rust, bypassing the
//! Python worker entirely.  The module loads the model once via `ort`, then
//! iterates over decoded frames, upscales each, and writes the result as PNG.

use std::{
    fs,
    path::{Path, PathBuf},
};

use helioframe_core::HelioFrameResult;
use ndarray::Array4;
use ort::{session::Session, value::Tensor};

use crate::worker::{WorkerLaunchConfig, WorkerRunResult};

/// Default model path for the fast-preview ONNX model.
const DEFAULT_MODEL_PATH: &str = "models/fast-preview/fast_preview_v1.onnx";

/// Run ONNX Runtime inference over every decoded frame in `config`.
///
/// The caller supplies a [`WorkerLaunchConfig`] identical to the one used by
/// the Python worker path.  The function:
///
/// 1. Loads the ONNX model from `model_path` (or the compile-time default).
/// 2. For each input frame PNG, decodes it, converts to an NCHW `f32` tensor,
///    runs the session, and writes the output frame as PNG.
/// 3. Returns a [`WorkerRunResult`] pointing at the output directory.
pub fn run_onnx_inference(
    config: &WorkerLaunchConfig<'_>,
    model_path: Option<&Path>,
) -> HelioFrameResult<WorkerRunResult> {
    let model_path = model_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MODEL_PATH));

    let worker_io_dir = config
        .run_layout
        .intermediate_artifacts_dir
        .join("onnx_worker");
    let output_frames_dir = worker_io_dir.join("frames");
    fs::create_dir_all(&output_frames_dir).map_err(|err| {
        helioframe_core::HelioFrameError::Config(format!(
            "failed to create ONNX worker output directory {}: {err}",
            output_frames_dir.display()
        ))
    })?;

    tracing::info!(
        model = %model_path.display(),
        frames = config.frame_count,
        "loading ONNX fast-preview model"
    );

    let mut session = Session::builder()
        .map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to create ONNX session builder: {err}"
            ))
        })?
        .commit_from_file(&model_path)
        .map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to load ONNX model from {}: {err}",
                model_path.display()
            ))
        })?;

    for index in 0..config.frame_count {
        let frame_name = format!("frame_{index:010}.png");
        let input_path = config.input_frames_dir.join(&frame_name);
        let output_path = output_frames_dir.join(&frame_name);

        let input_array = load_frame_as_nchw(&input_path)?;
        let (n, c, h, w) = input_array.dim();
        let shape = vec![n as i64, c as i64, h as i64, w as i64];
        let data = input_array.into_raw_vec_and_offset().0;

        let input_tensor = Tensor::from_array((shape, data.into_boxed_slice())).map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to create ONNX input tensor for frame {index}: {err}"
            ))
        })?;

        let outputs = session
            .run(ort::inputs![input_tensor])
            .map_err(|err| {
                helioframe_core::HelioFrameError::Config(format!(
                    "ONNX inference failed on frame {index}: {err}"
                ))
            })?;

        let (out_shape, out_data) = outputs[0].try_extract_tensor::<f32>().map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to extract ONNX output tensor for frame {index}: {err}"
            ))
        })?;

        save_nchw_as_frame(&out_shape, out_data, &output_path)?;

        if index > 0 && index % 100 == 0 {
            tracing::debug!(frame = index, "ONNX fast-preview progress");
        }
    }

    tracing::info!(
        frames = config.frame_count,
        "ONNX fast-preview inference complete"
    );

    Ok(WorkerRunResult {
        output_frames_dir,
        frame_count: config.frame_count,
        output_manifest_path: worker_io_dir.join("onnx-output.json"),
    })
}

/// Decode a PNG frame into an NCHW f32 array normalised to [0, 1].
fn load_frame_as_nchw(path: &Path) -> HelioFrameResult<Array4<f32>> {
    let img = image::open(path)
        .map_err(|err| {
            helioframe_core::HelioFrameError::Config(format!(
                "failed to open frame {}: {err}",
                path.display()
            ))
        })?
        .to_rgb8();

    let (width, height) = img.dimensions();
    let raw = img.into_raw();

    // HWC u8 -> NCHW f32 [0..1]
    let mut tensor = Array4::<f32>::zeros((1, 3, height as usize, width as usize));
    for y in 0..height as usize {
        for x in 0..width as usize {
            let base = (y * width as usize + x) * 3;
            tensor[[0, 0, y, x]] = raw[base] as f32 / 255.0;
            tensor[[0, 1, y, x]] = raw[base + 1] as f32 / 255.0;
            tensor[[0, 2, y, x]] = raw[base + 2] as f32 / 255.0;
        }
    }

    Ok(tensor)
}

/// Convert an NCHW f32 flat buffer back to an RGB PNG on disk.
///
/// `shape` is expected to be `[1, 3, H, W]` or `[3, H, W]`.
fn save_nchw_as_frame(shape: &[i64], data: &[f32], path: &Path) -> HelioFrameResult<()> {
    let (channels, height, width) = if shape.len() == 4 {
        (shape[1] as usize, shape[2] as usize, shape[3] as usize)
    } else if shape.len() == 3 {
        (shape[0] as usize, shape[1] as usize, shape[2] as usize)
    } else {
        return Err(helioframe_core::HelioFrameError::Config(format!(
            "unexpected ONNX output tensor shape: {shape:?}"
        )));
    };

    if channels != 3 {
        return Err(helioframe_core::HelioFrameError::Config(format!(
            "expected 3-channel output, got {channels} channels"
        )));
    }

    let mut pixels = vec![0u8; height * width * 3];
    for y in 0..height {
        for x in 0..width {
            let px = (y * width + x) * 3;
            for ch in 0..3 {
                // NCHW layout: index = ((batch * C + ch) * H + y) * W + x
                let src_idx = if shape.len() == 4 {
                    ((ch) * height + y) * width + x
                } else {
                    (ch * height + y) * width + x
                };
                let v = data[src_idx].clamp(0.0, 1.0);
                pixels[px + ch] = (v * 255.0 + 0.5) as u8;
            }
        }
    }

    let img = image::RgbImage::from_raw(width as u32, height as u32, pixels).ok_or_else(|| {
        helioframe_core::HelioFrameError::Config(
            "failed to construct output image from tensor data".to_string(),
        )
    })?;

    img.save(path).map_err(|err| {
        helioframe_core::HelioFrameError::Config(format!(
            "failed to save output frame {}: {err}",
            path.display()
        ))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_and_save_roundtrip() {
        let tmp = std::env::temp_dir().join("helioframe-onnx-test");
        fs::create_dir_all(&tmp).unwrap();

        let test_img = image::RgbImage::from_fn(4, 4, |x, y| {
            image::Rgb([(x * 60) as u8, (y * 60) as u8, 128])
        });
        let input_path = tmp.join("test_frame.png");
        test_img.save(&input_path).unwrap();

        let tensor = load_frame_as_nchw(&input_path).unwrap();
        assert_eq!(tensor.shape(), &[1, 3, 4, 4]);
        assert!(tensor[[0, 0, 0, 0]] >= 0.0 && tensor[[0, 0, 0, 0]] <= 1.0);

        // Round-trip through the save function using the raw data.
        let (n, c, h, w) = tensor.dim();
        let shape = [n as i64, c as i64, h as i64, w as i64];
        let data = tensor.into_raw_vec_and_offset().0;
        let output_path = tmp.join("test_output.png");
        save_nchw_as_frame(&shape, &data, &output_path).unwrap();
        assert!(output_path.exists());

        let _ = fs::remove_dir_all(tmp);
    }
}
