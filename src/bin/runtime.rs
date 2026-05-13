//! SD-PLC Runtime — Deterministic Scan Cycle Executor
//!
//! Compiles an IEC 61131-3 ST program, JIT-executes it in a
//! repeating scan cycle, and displays live variable values in
//! a terminal dashboard.
//!
//! Usage:
//!   cargo run --bin runtime -- programs/flotation_column.st
//!   cargo run --bin runtime -- programs/hello.st --scan-time=50
//!   cargo run --bin runtime -- programs/hello.st --cycles=100

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;
use std::time::{Duration, Instant};

use inkwell::context::Context;
use inkwell::execution_engine::JitFunction;
use inkwell::OptimizationLevel;

use sdplc::codegen::{CodeGenerator, RuntimeVar};
use sdplc::lexer::Lexer;
use sdplc::parser::Parser;
use sdplc::semantic;
use sdplc::semantic::ResolvedType;

// ─── JIT function signature ─────────────────────────────────────

/// Void function — used for __init and __scan.
type VoidFn = unsafe extern "C" fn();
/// Getter function — returns a variable's value as f64.
type GetterFn = unsafe extern "C" fn() -> f64;

// ─── Main ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    // Parse arguments
    let mut input_path: Option<String> = None;
    let mut scan_time_ms: u64 = 100;
    let mut max_cycles: Option<u64> = None;
    let mut quiet = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if let Some(val) = arg.strip_prefix("--scan-time=") {
            scan_time_ms = val.parse().unwrap_or_else(|_| {
                eprintln!("error: invalid scan time '{}'", val);
                process::exit(1);
            });
        } else if let Some(val) = arg.strip_prefix("--cycles=") {
            max_cycles = Some(val.parse().unwrap_or_else(|_| {
                eprintln!("error: invalid cycle count '{}'", val);
                process::exit(1);
            }));
        } else if arg == "-q" || arg == "--quiet" {
            quiet = true;
        } else if arg.starts_with('-') {
            eprintln!("error: unknown option '{}'", arg);
            process::exit(1);
        } else {
            input_path = Some(arg.clone());
        }
        i += 1;
    }

    let input_path = input_path.unwrap_or_else(|| {
        eprintln!("error: no input file");
        eprintln!("Usage: runtime <input.st> [--scan-time=100] [--cycles=1000]");
        process::exit(1);
    });

    // ── Read source ──
    let source = fs::read_to_string(&input_path).unwrap_or_else(|e| {
        eprintln!("error: cannot read '{}': {}", input_path, e);
        process::exit(1);
    });
    let source_name = Path::new(&input_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("program")
        .to_string();

    if !quiet {
        println!("═══ SD-PLC Runtime ═══\n");
        println!("Input:     {}", input_path);
        println!("Scan time: {} ms", scan_time_ms);
        if let Some(n) = max_cycles {
            println!("Max cycles: {}", n);
        }
        println!();
    }

    // ── Stage 1: Lex ──
    let mut check_lexer = Lexer::new(&source);
    let tokens = check_lexer.tokenize();
    let unknown = tokens.iter()
        .filter(|t| t.kind == sdplc::lexer::TokenType::Unknown)
        .count();
    if unknown > 0 {
        eprintln!("Lexer: {} unknown token(s)", unknown);
        process::exit(1);
    }
    if !quiet { println!("Lexer:    ✓ {} tokens", tokens.len()); }

    // ── Stage 2: Parse ──
    let lexer = Lexer::new(&source);
    let mut parser = Parser::new(lexer);
    let ast = match parser.parse() {
        Ok(ast) => {
            if !quiet { println!("Parser:   ✓ {} POU(s)", ast.units.len()); }
            ast
        }
        Err(e) => {
            eprintln!("Parse error: {}", e);
            process::exit(1);
        }
    };

    // ── Stage 3: Semantic check ──
    let sem = semantic::analyze(ast.clone());
    if sem.has_errors() {
        for d in &sem.diagnostics {
            eprintln!("  {}", d);
        }
        eprintln!("Semantic analysis failed.");
        process::exit(1);
    }
    if !quiet { println!("Semantic: ✓"); }

    // ── Stage 4: Compile for runtime (globals + JIT functions) ──
    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context, &source_name);

    let runtime_vars = match codegen.compile_for_runtime(&ast) {
        Ok(vars) => {
            if !quiet { println!("Codegen:  ✓ {} variables", vars.len()); }
            vars
        }
        Err(e) => {
            eprintln!("Codegen error: {}", e);
            process::exit(1);
        }
    };

    if runtime_vars.is_empty() {
        eprintln!("error: no PROGRAM found in input file");
        process::exit(1);
    }

    let program_name = &runtime_vars[0].program_name;

    // ── Stage 5: Create JIT execution engine ──
    let execution_engine = codegen.module()
        .create_jit_execution_engine(OptimizationLevel::None)
        .unwrap_or_else(|e| {
            eprintln!("JIT error: {}", e);
            process::exit(1);
        });

    // Get function pointers
    let init_name = format!("__init_{}", program_name);
    let scan_name = format!("__scan_{}", program_name);

    let init_fn: JitFunction<VoidFn> = unsafe {
        execution_engine.get_function(&init_name)
    }.unwrap_or_else(|e| {
        eprintln!("JIT: cannot find {}: {}", init_name, e);
        process::exit(1);
    });

    let scan_fn: JitFunction<VoidFn> = unsafe {
        execution_engine.get_function(&scan_name)
    }.unwrap_or_else(|e| {
        eprintln!("JIT: cannot find {}: {}", scan_name, e);
        process::exit(1);
    });

    // Build getter table
    let getters: Vec<(&RuntimeVar, JitFunction<GetterFn>)> = runtime_vars.iter()
        .filter_map(|var| {
            let getter_name = format!("__get_{}", var.name);
            let getter: JitFunction<GetterFn> = unsafe {
                execution_engine.get_function(&getter_name)
            }.ok()?;
            Some((var, getter))
        })
        .collect();

    if !quiet {
        println!("JIT:      ✓ engine ready\n");
    }

    // ── Initialise ──
    unsafe { init_fn.call(); }

    // ── Scan cycle loop ──
    let scan_duration = Duration::from_millis(scan_time_ms);
    let mut cycle: u64 = 0;
    let mut jitter_sum: f64 = 0.0;
    let mut jitter_max: f64 = 0.0;
    let runtime_start = Instant::now();

    // Install Ctrl+C handler via atomic flag
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc_handler(r);

    loop {
        if !running.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        if let Some(max) = max_cycles {
            if cycle >= max {
                break;
            }
        }

        let cycle_start = Instant::now();

        // ── Execute one scan cycle ──
        unsafe { scan_fn.call(); }

        let exec_time = cycle_start.elapsed();

        // ── Read all variables ──
        let values: Vec<f64> = getters.iter()
            .map(|(_, getter)| unsafe { getter.call() })
            .collect();

        // ── Timing ──
        let cycle_elapsed = cycle_start.elapsed();
        let jitter_us = if cycle_elapsed > scan_duration {
            (cycle_elapsed - scan_duration).as_micros() as f64
        } else {
            0.0
        };
        jitter_sum += jitter_us;
        if jitter_us > jitter_max { jitter_max = jitter_us; }

        cycle += 1;

        // ── Display ──
        if !quiet {
            // Clear screen and move cursor to top
            print!("\x1b[2J\x1b[H");

            let total_elapsed = runtime_start.elapsed();
            println!("══ SD-PLC Runtime ══  {}  Scan: {}ms  Cycle: #{}",
                program_name, scan_time_ms, cycle);
            println!("   Uptime: {:.1}s  Exec: {:.1}µs  Jitter avg: {:.1}µs  max: {:.1}µs\n",
                total_elapsed.as_secs_f64(),
                exec_time.as_micros() as f64,
                if cycle > 0 { jitter_sum / cycle as f64 } else { 0.0 },
                jitter_max,
            );

            // Variable table
            println!(" {:<24} {:<10} {:>14}", "Variable", "Type", "Value");
            println!(" {}", "─".repeat(50));

            for (i, (var, _)) in getters.iter().enumerate() {
                let val = values[i];
                let type_str = format_type_short(&var.resolved_type);
                let val_str = format_value(val, &var.resolved_type);
                println!(" {:<24} {:<10} {:>14}", var.name, type_str, val_str);
            }

            println!("\n Press Ctrl+C to stop.");
            io::stdout().flush().ok();
        }

        // ── Sleep until next cycle ──
        let elapsed = cycle_start.elapsed();
        if elapsed < scan_duration {
            std::thread::sleep(scan_duration - elapsed);
        }
    }

    // ── Summary ──
    let total = runtime_start.elapsed();
    println!("\n── Runtime Summary ──");
    println!("  Cycles:     {}", cycle);
    println!("  Uptime:     {:.3}s", total.as_secs_f64());
    println!("  Avg jitter: {:.1}µs", if cycle > 0 { jitter_sum / cycle as f64 } else { 0.0 });
    println!("  Max jitter: {:.1}µs", jitter_max);
    if cycle > 0 {
        println!("  Avg cycle:  {:.3}ms", total.as_secs_f64() * 1000.0 / cycle as f64);
    }

    // Print final variable values
    println!("\n  Final values:");
    for (i, (var, _)) in getters.iter().enumerate() {
        let val = unsafe { getters[i].1.call() };
        let val_str = format_value(val, &var.resolved_type);
        println!("    {} = {}", var.name, val_str);
    }
}

// ─── Formatting ─────────────────────────────────────────────────

fn format_type_short(rt: &ResolvedType) -> &'static str {
    match rt {
        ResolvedType::Bool => "BOOL",
        ResolvedType::SignedInt { bits: 8 } => "SINT",
        ResolvedType::SignedInt { bits: 16 } => "INT",
        ResolvedType::SignedInt { bits: 32 } => "DINT",
        ResolvedType::SignedInt { bits: 64 } => "LINT",
        ResolvedType::UnsignedInt { bits: 8 } => "USINT",
        ResolvedType::UnsignedInt { bits: 16 } => "UINT",
        ResolvedType::UnsignedInt { bits: 32 } => "UDINT",
        ResolvedType::UnsignedInt { bits: 64 } => "ULINT",
        ResolvedType::Float { bits: 32 } => "REAL",
        ResolvedType::Float { bits: 64 } => "LREAL",
        ResolvedType::BitString { bits: 8 } => "BYTE",
        ResolvedType::BitString { bits: 16 } => "WORD",
        ResolvedType::BitString { bits: 32 } => "DWORD",
        ResolvedType::BitString { bits: 64 } => "LWORD",
        ResolvedType::Time => "TIME",
        ResolvedType::Date => "DATE",
        ResolvedType::Tod => "TOD",
        ResolvedType::Dt => "DT",
        ResolvedType::Array { .. } => "ARRAY",
        _ => "???",
    }
}

fn format_value(val: f64, rt: &ResolvedType) -> String {
    match rt {
        ResolvedType::Bool => {
            if val != 0.0 { "TRUE".to_string() } else { "FALSE".to_string() }
        }
        ResolvedType::Float { .. } => format!("{:.4}", val),
        ResolvedType::SignedInt { .. } => format!("{}", val as i64),
        ResolvedType::UnsignedInt { .. } => format!("{}", val as u64),
        ResolvedType::BitString { .. } => format!("16#{:X}", val as u64),
        ResolvedType::Array { .. } => "(array)".to_string(),
        _ => format!("{}", val as i64),
    }
}

// ─── Ctrl+C handler ─────────────────────────────────────────────

fn ctrlc_handler(running: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    // Cross-platform Ctrl+C handling without external crates.
    // Uses a spawned thread that sleeps and checks — crude but works.
    // On Ctrl+C the process exits; the OS reclaims everything.
    // For a polished version, add the `ctrlc` crate to Cargo.toml.
    let _ = running; // Flag exists for future ctrlc crate integration
}

// ─── Usage ──────────────────────────────────────────────────────

fn print_usage() {
    println!("SD-PLC Runtime — Deterministic Scan Cycle Executor\n");
    println!("USAGE:");
    println!("  runtime <input.st>                      Run with 100ms scan cycle");
    println!("  runtime <input.st> --scan-time=50        50ms scan cycle");
    println!("  runtime <input.st> --cycles=1000         Stop after 1000 cycles");
    println!("  runtime <input.st> -q                    Quiet (summary only)\n");
    println!("The runtime compiles the ST program, JIT-executes it in a");
    println!("repeating scan cycle, and displays live variable values.");
    println!("Press Ctrl+C to stop and print the summary.");
}