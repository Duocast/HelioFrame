use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use helioframe_core::{
    run_doctor, AppConfig, BackendKind, DoctorSummary, PresetConfig, Resolution, UpscalePreset,
};
use helioframe_pipeline::PipelineOrchestrator;
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
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        #[arg(long, default_value = "studio")]
        preset: String,
        #[arg(long)]
        backend: Option<String>,
        #[arg(long, default_value_t = 3840)]
        width: u32,
        #[arg(long, default_value_t = 2160)]
        height: u32,
    },
    Doctor,
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
            dry_run,
            preset,
            backend,
            width,
            height,
        } => run_upscale(input, output, dry_run, preset, backend, width, height),
        Commands::Doctor => run_doctor_command(),
    }
}

fn run_doctor_command() -> anyhow::Result<()> {
    let report = run_doctor();
    print_human_readable_report(&report);
    print_json_report(&report)?;

    if report.is_ok() {
        Ok(())
    } else {
        let failed = report
            .failed_checks()
            .into_iter()
            .map(|check| check.name)
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("doctor failed: missing or invalid dependencies ({failed})")
    }
}

fn print_human_readable_report(report: &DoctorSummary) {
    println!("HelioFrame doctor report");
    println!("Platform support: {}", report.platform_notice);
    println!();

    for check in &report.checks {
        let status = if check.passed { "PASS" } else { "FAIL" };
        println!("[{status}] {}", check.name);
        println!("  detail: {}", check.detail);
        if let Some(action) = &check.action {
            println!("  action: {action}");
        }
    }

    println!();
    println!(
        "Overall: {}",
        if report.is_ok() {
            "PASS"
        } else {
            "FAIL (action required)"
        }
    );
    println!();
}

fn print_json_report(report: &DoctorSummary) -> anyhow::Result<()> {
    println!("JSON summary:");
    let json = serde_json::to_string_pretty(report)?;
    println!("{json}");
    Ok(())
}

fn run_upscale(
    input: PathBuf,
    output: PathBuf,
    dry_run: bool,
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

    if dry_run {
        let plan = PipelineOrchestrator::plan(&config, preset_cfg)?;
        print_plan(&config, &plan);
        println!();
        println!("Dry run complete. No pipeline stages were executed.");
        return Ok(());
    }

    let execution = PipelineOrchestrator::execute(&config, preset_cfg)?;
    let plan = &execution.plan;

    print_plan(&config, plan);
    println!();
    println!("Run ID: {}", execution.run_layout.run_id);
    println!("Run directory: {}", execution.run_layout.run_dir.display());
    println!("Manifest: {}", execution.run_layout.manifest_path.display());

    Ok(())
}

fn print_plan(config: &AppConfig, plan: &helioframe_pipeline::ExecutionPlan) {
    info!("HelioFrame execution plan");
    println!("Input:   {}", config.input);
    println!("Output:  {}", config.output);
    println!("Preset:  {}", config.preset);
    println!("Backend: {}", config.backend);
    println!("Target:  {}", config.target_resolution);
    println!("Source container: {}", plan.probe.container);
    println!(
        "Assumed source resolution: {}",
        plan.probe.assumed_resolution
    );
    println!("Model summary: {}", plan.inference.summary);
    println!();
    println!("Pipeline stages:");
    for stage in &plan.stages {
        println!("- {:<12} {}", stage.name, stage.description);
    }
    println!();
    println!("Preset details:");
    println!(
        "- temporal_window:          {}",
        plan.preset.temporal_window
    );
    println!("- tile_size:                {}", plan.preset.tile_size);
    println!("- overlap:                  {}", plan.preset.overlap);
    println!(
        "- diffusion_steps:          {}",
        plan.preset.diffusion_steps
    );
    println!(
        "- fp16:                     {}",
        plan.preset.use_half_precision
    );
    println!(
        "- patch-wise 4K enabled:    {}",
        plan.preset.enable_patchwise_4k
    );
    println!(
        "- structural guidance:      {}",
        plan.preset.enable_structural_guidance
    );
    println!(
        "- detail refiner:           {}",
        plan.preset.enable_detail_refiner
    );
    println!(
        "- temporal checks:          {}",
        plan.preset.enable_temporal_consistency_checks
    );
    println!(
        "- reject on regression:     {}",
        plan.preset.reject_on_temporal_regression
    );
    println!(
        "- anchor_frame_stride:      {}",
        plan.preset.anchor_frame_stride
    );
    println!();
    println!("Inference hints:");
    println!(
        "- patch-wise 4K:            {}",
        plan.inference.hints.patch_wise_4k
    );
    println!(
        "- multi-step diffusion:     {}",
        plan.inference.hints.multi_step_diffusion
    );
    println!(
        "- structural guidance:      {}",
        plan.inference.hints.structural_guidance
    );
    println!(
        "- detail refiner:           {}",
        plan.inference.hints.detail_refiner
    );
    println!(
        "- temporal QC gate:         {}",
        plan.inference.hints.temporal_qc_gate
    );
    println!(
        "- teacher-guided:           {}",
        plan.inference.hints.teacher_guided
    );
    println!(
        "- custom kernels advised:   {}",
        plan.inference.hints.custom_kernels_recommended
    );
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
