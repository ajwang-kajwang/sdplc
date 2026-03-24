//! SD-PLC Compiler Driver
//!
//! Usage:
//!   sdplc <input.st>                  Compile ST file → output.ll + output.bc
//!   sdplc <input.st> -o <name>        Compile with custom output name
//!   sdplc <input.st> --emit-ir        Print LLVM IR to stdout (no files)
//!   sdplc                             Run built-in demo program
//!   sdplc --help                      Show usage
//!
//! Reads IEC 61131-3 Structured Text from a .st or .txt file and
//! compiles it through four stages: Lex → Parse → Semantic → LLVM IR.

use std::env;
use std::fs;
use std::path::Path;
use std::process;

use sdplc::codegen::CodeGenerator;
use sdplc::lexer::{Lexer, TokenType};
use sdplc::parser::Parser;
use sdplc::semantic;

use inkwell::context::Context;

fn main() {
    let args: Vec<String> = env::args().collect();

    // ── CLI arguments ──
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    let mut input_path: Option<String> = None;
    let mut output_name: Option<String> = None;
    let mut emit_ir_only = false;
    let mut quiet = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--emit-ir" => emit_ir_only = true,
            "-q" | "--quiet" => quiet = true,
            "-o" => {
                i += 1;
                if i < args.len() {
                    output_name = Some(args[i].clone());
                } else {
                    eprintln!("error: -o requires an argument");
                    process::exit(1);
                }
            }
            arg if arg.starts_with('-') => {
                eprintln!("error: unknown option '{}'", arg);
                eprintln!("Try 'sdplc --help' for usage.");
                process::exit(1);
            }
            _ => {
                input_path = Some(args[i].clone());
            }
        }
        i += 1;
    }

    // ── Load source code ──
    let (source_code, source_name) = match input_path {
        Some(ref path) => {
            let source = fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("error: cannot read '{}': {}", path, e);
                process::exit(1);
            });
            let name = Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("output")
                .to_string();
            (source, name)
        }
        None => {
            if !quiet {
                eprintln!("No input file — running built-in demo.\n\
                           Use 'sdplc <file.st>' to compile a file.\n\
                           Use 'sdplc --help' for all options.\n");
            }
            (DEMO_PROGRAM.to_string(), "demo".to_string())
        }
    };

    let out_base = output_name.unwrap_or(source_name.clone());

    if !quiet {
        println!("═══ SD-PLC Compiler ═══\n");
        if input_path.is_some() {
            println!("Input:  {}", input_path.as_ref().unwrap());
        }
        println!("Output: {}.ll, {}.bc\n", out_base, out_base);
    }

    // ── Stage 1: Lexing ──
    let mut token_lexer = Lexer::new(&source_code);
    let tokens = token_lexer.tokenize();
    let unknown_count = tokens.iter()
        .filter(|t| t.kind == TokenType::Unknown)
        .count();

    if !quiet {
        println!("Stage 1 — Lexer");
        println!("  {} chars → {} tokens", source_code.len(), tokens.len());
    }
    if unknown_count > 0 {
        eprintln!("  ✗ {} unknown token(s):", unknown_count);
        for t in tokens.iter().filter(|t| t.kind == TokenType::Unknown) {
            eprintln!("    [{}:{}] '{}'", t.line, t.col, t.text);
        }
        process::exit(1);
    }
    if !quiet { println!("  ✓ Complete.\n"); }

    // ── Stage 2: Parsing ──
    let lexer = Lexer::new(&source_code);
    let mut parser = Parser::new(lexer);
    let ast = match parser.parse() {
        Ok(ast) => {
            if !quiet {
                println!("Stage 2 — Parser");
                println!("  ✓ {} POU(s)\n", ast.units.len());
            }
            ast
        }
        Err(e) => {
            eprintln!("Parse error: {}", e);
            process::exit(1);
        }
    };

    // ── Stage 3: Semantic Analysis ──
    let sem_ctx = semantic::analyze(ast.clone());

    if !quiet {
        println!("Stage 3 — Semantic Analysis");
        println!("  {} error(s), {} warning(s)", sem_ctx.error_count(), sem_ctx.warning_count());
    }
    for d in &sem_ctx.diagnostics {
        if !quiet || d.severity == semantic::Severity::Error {
            eprintln!("  {}", d);
        }
    }
    if sem_ctx.has_errors() {
        eprintln!("Compilation aborted.");
        process::exit(1);
    }
    if !quiet { println!("  ✓ Complete.\n"); }

    // ── Stage 4: LLVM IR Generation ──
    let llvm_context = Context::create();
    let mut codegen = CodeGenerator::new(&llvm_context, &source_name);

    match codegen.compile(&ast) {
        Ok(()) => {
            if emit_ir_only {
                // Print IR to stdout and exit — useful for piping
                print!("{}", codegen.ir_string());
                return;
            }

            if !quiet {
                println!("Stage 4 — LLVM IR Generation");
                println!("  ✓ IR emitted.\n");
            }

            // Write output files
            let ll_path = format!("{}.ll", out_base);
            let bc_path = format!("{}.bc", out_base);

            match codegen.write_ir(&ll_path) {
                Ok(()) => {
                    if !quiet { println!("  → {}", ll_path); }
                }
                Err(e) => eprintln!("  ✗ Failed to write {}: {}", ll_path, e),
            }
            if codegen.write_bitcode(&bc_path) {
                if !quiet { println!("  → {}", bc_path); }
            }

            if !quiet {
                println!("\n── Cross-Compilation ──");
                println!("  x86_64:     llc {ll} -o {base}.s", ll = ll_path, base = out_base);
                println!("  ARMv8:      llc {ll} -mtriple=aarch64-linux-gnu -o {base}_arm64.s",
                    ll = ll_path, base = out_base);
                println!("  ARMv5TE:    llc {ll} -mtriple=armv5te-linux-gnueabi -o {base}_armv5.s",
                    ll = ll_path, base = out_base);
                println!("  WebAssembly: llc {ll} -mtriple=wasm32-unknown-unknown -o {base}.wasm",
                    ll = ll_path, base = out_base);
            }
        }
        Err(e) => {
            eprintln!("Codegen error: {}", e);
            process::exit(1);
        }
    }
}

fn print_usage() {
    println!("SD-PLC — IEC 61131-3 Structured Text Compiler\n");
    println!("USAGE:");
    println!("  sdplc <input.st>               Compile to LLVM IR");
    println!("  sdplc <input.st> -o <name>     Custom output basename");
    println!("  sdplc <input.st> --emit-ir     Print IR to stdout");
    println!("  sdplc -q <input.st>            Quiet mode (errors only)");
    println!("  sdplc                          Run built-in demo");
    println!("  sdplc --help                   Show this message\n");
    println!("INPUT:");
    println!("  Any .st or .txt file containing IEC 61131-3 Structured Text.");
    println!("  Multiple POUs (PROGRAM, FUNCTION, FUNCTION_BLOCK) per file.\n");
    println!("OUTPUT:");
    println!("  <name>.ll    LLVM IR text (human-readable)");
    println!("  <name>.bc    LLVM bitcode (for llc/opt)\n");
    println!("EXAMPLES:");
    println!("  sdplc conveyor.st");
    println!("  sdplc conveyor.st -o build/conveyor");
    println!("  sdplc pid.st --emit-ir | llc -o pid.s");
    println!("  cat motor.st | sdplc /dev/stdin -o motor");
}

/// Built-in demonstration program used when no input file is given.
const DEMO_PROGRAM: &str = r#"
PROGRAM ConveyorControl
VAR
    speed : REAL := 0.0;
    running : BOOL := FALSE;
    count : INT := 0;
    limit : INT := 1000;
    i : INT;
    sensor_vals : ARRAY[0..7] OF DINT;
END_VAR

(* Main conveyor control logic *)
IF running AND speed > 0.0 THEN
    count := count + 1;
    IF count >= limit THEN
        running := FALSE;
        speed := 0.0;
    ELSIF speed > 100.0 THEN
        speed := 100.0;
    END_IF;
END_IF;

FOR i := 0 TO 7 BY 1 DO
    sensor_vals[i] := sensor_vals[i] + 1;
END_FOR;

CASE count MOD 4 OF
    0: speed := 25.0;
    1..2: speed := 50.0;
    3: speed := 75.0;
END_CASE;

END_PROGRAM
"#;