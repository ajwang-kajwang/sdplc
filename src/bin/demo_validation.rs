//! Demo validation pack runner.
//!
//! This orchestration binary produces the repeatable CSV and Markdown
//! artefacts called out in docs/sprint_roadmap.md. It intentionally runs
//! the public binaries through Cargo so the evidence matches the commands a
//! thesis reader or examiner can repeat.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

use inkwell::context::Context;
use sdplc::codegen::CodeGenerator;
use sdplc::lexer::{Lexer, TokenType};
use sdplc::parser::Parser;
use sdplc::semantic;

#[derive(Debug, Clone)]
struct Config {
    cycles: u64,
    output_dir: PathBuf,
    quick: bool,
    skip_opcua: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cycles: 1000,
            output_dir: PathBuf::from("results"),
            quick: false,
            skip_opcua: false,
        }
    }
}

#[derive(Debug, Clone)]
struct CommandResult {
    label: String,
    command: String,
    elapsed_s: f64,
    success: bool,
}

#[derive(Debug, Clone)]
struct CompilerMetric {
    benchmark: String,
    source: String,
    phase: String,
    elapsed_us: f64,
    items: usize,
    status: String,
}

fn main() {
    let config = parse_args();
    if let Err(err) = run(config) {
        eprintln!("Demo validation failed: {err}");
        std::process::exit(1);
    }
}

fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(&config.output_dir)?;
    let cycles = if config.quick { 5 } else { config.cycles };
    let opcua_read_count = if config.quick { 5 } else { 1000 };
    let opcua_write_count = if config.quick { 3 } else { 100 };

    let mut results = Vec::new();

    let mut compiler_metrics = Vec::new();
    compiler_metrics.extend(measure_compiler_pipeline(
        "compiler flotation",
        "examples/flotation_tank.st",
    )?);
    compiler_metrics.extend(measure_compiler_pipeline(
        "compiler control_flow",
        "programs/control_flow.st",
    )?);
    write_compiler_benchmark_csv(&config.output_dir, &compiler_metrics)?;

    let compiler_flotation = compiler_benchmark_command(
        "compiler flotation",
        "examples/flotation_tank.st",
        &config.output_dir,
    )?;
    let compiler_control_flow = compiler_benchmark_command(
        "compiler control_flow",
        "programs/control_flow.st",
        &config.output_dir,
    )?;
    results.push(compiler_flotation);
    results.push(compiler_control_flow);

    for scan_ms in [10_u64, 20, 50] {
        results.push(run_runtime_benchmark(
            &format!("flotation runtime {scan_ms}ms"),
            "examples/flotation_tank.st",
            cycles,
            scan_ms,
            &config.output_dir,
            &format!("scan_timing_{scan_ms}ms.csv"),
            (scan_ms == 10).then_some("flotation_tank_telemetry.csv"),
        )?);
    }

    results.push(run_runtime_benchmark(
        "control_flow runtime 10ms",
        "programs/control_flow.st",
        cycles,
        10,
        &config.output_dir,
        "control_flow_scan_timing_10ms.csv",
        None,
    )?);

    if !config.skip_opcua {
        results.push(run_opcua_benchmark(
            &config.output_dir,
            opcua_read_count,
            opcua_write_count,
        )?);
    }

    write_summary(&config.output_dir, cycles, &results, config.skip_opcua)?;

    println!("Demo validation pack complete");
    println!("  output: {}", config.output_dir.display());
    println!(
        "  summary: {}",
        config.output_dir.join("validation_summary.md").display()
    );
    Ok(())
}

fn measure_compiler_pipeline(
    label: &str,
    source_path: &str,
) -> Result<Vec<CompilerMetric>, Box<dyn std::error::Error>> {
    let source = fs::read_to_string(source_path)?;
    let source_name = Path::new(source_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("program");
    let mut metrics = Vec::new();

    let started = Instant::now();
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();
    let unknown = tokens
        .iter()
        .filter(|t| t.kind == TokenType::Unknown)
        .count();
    if unknown > 0 {
        return Err(format!("{source_path} has {unknown} unknown token(s)").into());
    }
    metrics.push(CompilerMetric::new(
        label,
        source_path,
        "lex",
        started.elapsed().as_secs_f64() * 1_000_000.0,
        tokens.len(),
    ));

    let started = Instant::now();
    let lexer = Lexer::new(&source);
    let mut parser = Parser::new(lexer);
    let ast = parser
        .parse()
        .map_err(|err| format!("{source_path} parse failed: {err}"))?;
    metrics.push(CompilerMetric::new(
        label,
        source_path,
        "parse",
        started.elapsed().as_secs_f64() * 1_000_000.0,
        ast.units.len(),
    ));

    let started = Instant::now();
    let sem = semantic::analyze(ast.clone());
    if sem.has_errors() {
        return Err(format!("{source_path} failed semantic analysis").into());
    }
    metrics.push(CompilerMetric::new(
        label,
        source_path,
        "semantic",
        started.elapsed().as_secs_f64() * 1_000_000.0,
        sem.diagnostics.len(),
    ));

    let started = Instant::now();
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context, source_name);
    codegen
        .compile(&ast)
        .map_err(|err| format!("{source_path} codegen failed: {err}"))?;
    metrics.push(CompilerMetric::new(
        label,
        source_path,
        "codegen",
        started.elapsed().as_secs_f64() * 1_000_000.0,
        codegen.ir_string().len(),
    ));

    Ok(metrics)
}

impl CompilerMetric {
    fn new(benchmark: &str, source: &str, phase: &str, elapsed_us: f64, items: usize) -> Self {
        Self {
            benchmark: benchmark.to_string(),
            source: source.to_string(),
            phase: phase.to_string(),
            elapsed_us,
            items,
            status: "pass".to_string(),
        }
    }
}

fn compiler_benchmark_command(
    label: &str,
    source: &str,
    output_dir: &Path,
) -> Result<CommandResult, Box<dyn std::error::Error>> {
    let output_base = output_dir.join(Path::new(source).file_stem().unwrap());
    run_cargo(
        label,
        &[
            "run",
            "--quiet",
            "--bin",
            "sdplc",
            "--",
            source,
            "-o",
            output_base.to_str().ok_or("non-UTF-8 output path")?,
            "-q",
        ],
    )
}

fn run_runtime_benchmark(
    label: &str,
    source: &str,
    cycles: u64,
    scan_ms: u64,
    output_dir: &Path,
    timing_filename: &str,
    telemetry_filename: Option<&str>,
) -> Result<CommandResult, Box<dyn std::error::Error>> {
    let temp_dir = output_dir.join(format!(
        ".{}",
        timing_filename.trim_end_matches(".csv").replace('_', "-")
    ));
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)?;
    }
    fs::create_dir_all(&temp_dir)?;

    let result = run_cargo(
        label,
        &[
            "run",
            "--quiet",
            "--bin",
            "runtime",
            "--",
            source,
            &format!("--cycles={cycles}"),
            &format!("--scan-time={scan_ms}"),
            &format!("--out={}", temp_dir.display()),
            "-q",
        ],
    )?;

    fs::copy(
        temp_dir.join("runtime_scan_timing.csv"),
        output_dir.join(timing_filename),
    )?;
    fs::remove_dir_all(&temp_dir)?;

    if telemetry_filename.is_some() {
        run_validate_sim(cycles, scan_ms, output_dir, telemetry_filename)?;
    }

    Ok(result)
}

fn run_validate_sim(
    cycles: u64,
    scan_ms: u64,
    output_dir: &Path,
    telemetry_filename: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = output_dir.join(format!(".telemetry-{scan_ms}ms"));
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)?;
    }
    fs::create_dir_all(&temp_dir)?;

    run_cargo(
        &format!("flotation telemetry {scan_ms}ms"),
        &[
            "run",
            "--quiet",
            "--bin",
            "validate_sim",
            "--",
            &format!("--cycles={cycles}"),
            &format!("--scan-time={scan_ms}"),
            &format!("--out={}", temp_dir.display()),
        ],
    )?;

    if let Some(filename) = telemetry_filename {
        fs::copy(
            temp_dir.join("flotation_tank_telemetry.csv"),
            output_dir.join(filename),
        )?;
    }
    fs::remove_dir_all(&temp_dir)?;
    Ok(())
}

fn run_opcua_benchmark(
    output_dir: &Path,
    read_count: usize,
    write_count: usize,
) -> Result<CommandResult, Box<dyn std::error::Error>> {
    run_cargo(
        "opcua read/write latency",
        &[
            "run",
            "--quiet",
            "--bin",
            "opcua_server",
            "--",
            "examples/flotation_tank.st",
            "--scan-time=10",
            "--self-test",
            &format!("--read-count={read_count}"),
            &format!("--write-count={write_count}"),
            &format!("--out={}", output_dir.display()),
        ],
    )
}

fn run_cargo(label: &str, args: &[&str]) -> Result<CommandResult, Box<dyn std::error::Error>> {
    let command_line = format!("cargo {}", args.join(" "));
    println!("Running {label}: {command_line}");
    let started = Instant::now();
    let output = Command::new("cargo").args(args).output()?;
    let elapsed_s = started.elapsed().as_secs_f64();

    if !output.status.success() {
        return Err(format_command_error(label, &command_line, &output).into());
    }

    Ok(CommandResult {
        label: label.to_string(),
        command: command_line,
        elapsed_s,
        success: true,
    })
}

fn format_command_error(label: &str, command: &str, output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!(
        "{label} failed\ncommand: {command}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status, stdout, stderr
    )
}

fn write_compiler_benchmark_csv(
    output_dir: &Path,
    metrics: &[CompilerMetric],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = vec!["benchmark,source,phase,status,elapsed_us,items".to_string()];
    rows.extend(metrics.iter().map(|metric| {
        format!(
            "{},{},{},{},{:.3},{}",
            csv_field(&metric.benchmark),
            csv_field(&metric.source),
            csv_field(&metric.phase),
            csv_field(&metric.status),
            metric.elapsed_us,
            metric.items
        )
    }));
    fs::write(
        output_dir.join("compiler_pipeline_benchmark.csv"),
        rows.join("\n") + "\n",
    )?;
    Ok(())
}

fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn write_summary(
    output_dir: &Path,
    cycles: u64,
    results: &[CommandResult],
    skip_opcua: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut markdown = String::new();
    markdown.push_str("# SD-PLC Sprint 4 Validation Summary\n\n");
    markdown.push_str("## Scope\n\n");
    markdown.push_str(&format!(
        "This run produced repeatable evidence for compiler compilation, deterministic runtime scan timing, flotation-tank telemetry, and {} OPC UA read/write latency.\n\n",
        if skip_opcua { "skipped" } else { "wire-level" }
    ));
    markdown.push_str(&format!("Runtime cycle count: `{cycles}`.\n\n"));

    markdown.push_str("## Artefacts\n\n");
    markdown.push_str("| Artefact | Purpose |\n|---|---|\n");
    for (file, purpose) in [
        (
            "scan_timing_10ms.csv",
            "Flotation runtime scan timing at 10 ms",
        ),
        (
            "scan_timing_20ms.csv",
            "Flotation runtime scan timing at 20 ms",
        ),
        (
            "scan_timing_50ms.csv",
            "Flotation runtime scan timing at 50 ms",
        ),
        (
            "compiler_pipeline_benchmark.csv",
            "Compiler pipeline phase timings",
        ),
        (
            "control_flow_scan_timing_10ms.csv",
            "Control-flow ST runtime scan timing at 10 ms",
        ),
        (
            "flotation_tank_telemetry.csv",
            "Deterministic flotation-tank telemetry trend",
        ),
        (
            "opcua_read_latency.csv",
            "OPC UA read response latency samples",
        ),
        (
            "opcua_write_latency.csv",
            "OPC UA write response latency samples",
        ),
        (
            "validation_summary.md",
            "Thesis-ready summary of the validation run",
        ),
    ] {
        let status = if file == "validation_summary.md" || output_dir.join(file).exists() {
            "present"
        } else {
            "missing"
        };
        markdown.push_str(&format!("| `{file}` | {purpose} ({status}) |\n"));
    }

    markdown.push_str("\n## Commands\n\n");
    markdown.push_str("| Step | Result | Wall time (s) | Command |\n|---|---|---:|---|\n");
    for result in results {
        markdown.push_str(&format!(
            "| {} | {} | {:.3} | `{}` |\n",
            result.label,
            if result.success { "pass" } else { "fail" },
            result.elapsed_s,
            result.command
        ));
    }

    markdown.push_str("\n## Thesis Claim Boundary\n\n");
    markdown.push_str("Evidence in this folder supports claims for a Rust ST compiler frontend, representative semantic/code generation support, a deterministic scan-cycle runtime prototype, a flotation-tank validation case study, and OPC UA read/write exposure of process variables. It does not claim full O-PAS compliance.\n");

    fs::write(output_dir.join("validation_summary.md"), markdown)?;
    Ok(())
}

fn parse_args() -> Config {
    let mut config = Config::default();

    for arg in env::args().skip(1) {
        if let Some(value) = arg.strip_prefix("--cycles=") {
            config.cycles = value.parse().expect("--cycles must be a count");
        } else if let Some(value) = arg.strip_prefix("--out=") {
            config.output_dir = PathBuf::from(value);
        } else if arg == "--quick" {
            config.quick = true;
        } else if arg == "--skip-opcua" {
            config.skip_opcua = true;
        } else if arg == "--help" || arg == "-h" {
            print_help_and_exit();
        } else {
            eprintln!("unknown argument: {arg}");
            print_help_and_exit();
        }
    }

    config
}

fn print_help_and_exit() -> ! {
    println!("SD-PLC Sprint 4 validation pack\n");
    println!("USAGE:");
    println!("  cargo run --bin sprint4_validation -- --cycles=1000");
    println!();
    println!("OPTIONS:");
    println!("  --cycles=N     Runtime scan cycles, default 1000");
    println!("  --out=DIR      Output directory, default results");
    println!("  --quick        Short smoke run for development");
    println!("  --skip-opcua   Skip the OPC UA wire latency benchmark");
    std::process::exit(0);
}
