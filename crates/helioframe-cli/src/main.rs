use std::{path::PathBuf, str::FromStr};

use anyhow::Context;
use helioframe_core::{AppConfig, BackendKind, PresetConfig, Resolution, UpscalePreset};
use helioframe_pipeline::PipelineOrchestrator;
use clap::{Parser, Subcommand};
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "helioframe")]
#[command(about = "Rust-first scaffold for quality-first 4K video super-resolution")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Upscale {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value = "studio")]
        preset: String,
        #[arg(long)]
        backend: Option<String>,
        #[arg(long, default_value_t = 3840)]
        width: u32,
        #[arg(long, default_value_t = 2160)]
        height: u32,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .without_time()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Upscale {
            input,
            output,
            preset,
            backend,
            width,
            height,
        } => run_upscale(input, output, preset, backend, width, height),
    }
}

fn run_upscale(
    input: PathBuf,
    output: PathBuf,
    preset: String,
    backend: Option<String>,
    width: u32,
    height: u32,
) -> anyhow::Result<()> {
    let preset = parse_preset(&preset)?;

    let preset_path = match preset {
        UpscalePreset::Preview => "configs/presets/preview.toml",
        UpscalePreset::Balanced => "configs/presets/balanced.toml",
        UpscalePreset::Studio => "configs/presets/studio.toml",
        UpscalePreset::Experimental => "configs/presets/experimental.toml",
    };

    let preset_cfg = PresetConfig::load_from_file(preset_path)
        .with_context(|| format!("unable to load preset from {preset_path}"))?;

    let backend = backend
        .as_deref()
        .map(parse_backend)
        .transpose()?
        .unwrap_or(preset_cfg.default_backend);

    let config = AppConfig {
        input: input.display().to_string(),
        output: output.display().to_string(),
        backend,
        preset,
        target_resolution: Resolution { width, height },
    };

    let plan = PipelineOrchestrator::plan(&config, preset_cfg)?;

    info!("HelioFrame execution plan");
    println!("Input:   {}", config.input);
    println!("Output:  {}", config.output);
    println!("Preset:  {}", config.preset);
    println!("Backend: {}", config.backend);
    println!("Target:  {}", config.target_resolution);
    println!("Source container: {}", plan.probe.container);
    println!("Assumed source resolution: {}", plan.probe.assumed_resolution);
    println!("Model summary: {}", plan.inference.summary);
    println!();
    println!("Pipeline stages:");
    for stage in &plan.stages {
        println!("- {:<12} {}", stage.name, stage.description);
    }
    println!();
    println!("Preset details:");
    println!("- temporal_window:          {}", plan.preset.temporal_window);
    println!("- tile_size:                {}", plan.preset.tile_size);
    println!("- overlap:                  {}", plan.preset.overlap);
    println!("- diffusion_steps:          {}", plan.preset.diffusion_steps);
    println!("- fp16:                     {}", plan.preset.use_half_precision);
    println!("- patch-wise 4K enabled:    {}", plan.preset.enable_patchwise_4k);
    println!("- structural guidance:      {}", plan.preset.enable_structural_guidance);
    println!("- detail refiner:           {}", plan.preset.enable_detail_refiner);
    println!("- temporal checks:          {}", plan.preset.enable_temporal_consistency_checks);
    println!("- reject on regression:     {}", plan.preset.reject_on_temporal_regression);
    println!("- anchor_frame_stride:      {}", plan.preset.anchor_frame_stride);
    println!();
    println!("Inference hints:");
    println!("- patch-wise 4K:            {}", plan.inference.hints.patch_wise_4k);
    println!("- multi-step diffusion:     {}", plan.inference.hints.multi_step_diffusion);
    println!("- structural guidance:      {}", plan.inference.hints.structural_guidance);
    println!("- detail refiner:           {}", plan.inference.hints.detail_refiner);
    println!("- temporal QC gate:         {}", plan.inference.hints.temporal_qc_gate);
    println!("- teacher-guided:           {}", plan.inference.hints.teacher_guided);
    println!(
        "- custom kernels advised:   {}",
        plan.inference.hints.custom_kernels_recommended
    );

    Ok(())
}

fn parse_preset(value: &str) -> anyhow::Result<UpscalePreset> {
    match value {
        "preview" => Ok(UpscalePreset::Preview),
        "balanced" => Ok(UpscalePreset::Balanced),
        "studio" => Ok(UpscalePreset::Studio),
        "experimental" => Ok(UpscalePreset::Experimental),
        other => anyhow::bail!("unknown preset: {other}"),
    }
}

fn parse_backend(value: &str) -> anyhow::Result<BackendKind> {
    match value {
        "classical-baseline" => Ok(BackendKind::ClassicalBaseline),
        "fast-preview" => Ok(BackendKind::FastPreview),
        "seedvr-teacher" => Ok(BackendKind::SeedvrTeacher),
        "stcdit-studio" => Ok(BackendKind::StcditStudio),
        "helioframe-master" => Ok(BackendKind::HelioFrameMaster),
        other => anyhow::bail!("unknown backend: {other}"),
    }
}

impl FromStr for UpscalePreset {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_preset(s)
    }
}

impl FromStr for BackendKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_backend(s)
    }
}
