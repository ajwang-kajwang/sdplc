use sdplc::ast::*;
use sdplc::codegen::CodeGenerator;
use sdplc::lexer::Lexer;
use sdplc::parser::Parser;
use sdplc::semantic;

use inkwell::context::Context;

fn main() {
    let source_code = r#"
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

    println!("═══ SD-PLC Compiler ═══\n");

    // ── Stage 1: Lexing ──
    let mut token_lexer = Lexer::new(source_code);
    let tokens = token_lexer.tokenize();
    let unknown_count = tokens.iter()
        .filter(|t| t.kind == sdplc::lexer::TokenType::Unknown)
        .count();

    println!("Stage 1 — Lexer");
    println!("  {} chars → {} tokens", source_code.len(), tokens.len());
    if unknown_count > 0 {
        println!("  ✗ {} unknown token(s)", unknown_count);
        return;
    }
    println!("  ✓ Complete.\n");

    // ── Stage 2: Parsing ──
    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let ast = match parser.parse() {
        Ok(ast) => {
            println!("Stage 2 — Parser");
            println!("  ✓ AST: {} POU(s)\n", ast.units.len());
            ast
        }
        Err(e) => {
            println!("Stage 2 — Parser");
            println!("  ✗ {}\n", e);
            return;
        }
    };

    // ── Stage 3: Semantic Analysis ──
    let sem_ast = ast.clone();
    let ctx = semantic::analyze(sem_ast);

    println!("Stage 3 — Semantic Analysis");
    println!("  {} error(s), {} warning(s)", ctx.error_count(), ctx.warning_count());
    for d in &ctx.diagnostics {
        println!("  {}", d);
    }
    if ctx.has_errors() {
        println!("  ✗ Aborting.\n");
        return;
    }
    println!("  ✓ Complete.\n");

    // ── Stage 4: LLVM IR Generation ──
    let llvm_context = Context::create();
    let mut codegen = CodeGenerator::new(&llvm_context, "sdplc_conveyor");

    match codegen.compile(&ast) {
        Ok(()) => {
            println!("Stage 4 — LLVM IR Generation");
            println!("  ✓ IR emitted.\n");

            // Print IR
            println!("── Generated LLVM IR ──────────────────────────\n");
            let ir = codegen.ir_string();
            println!("{}", ir);

            // Write to files
            match codegen.write_ir("output.ll") {
                Ok(()) => println!("  → Wrote output.ll"),
                Err(e) => println!("  ✗ Failed to write .ll: {}", e),
            }
            if codegen.write_bitcode("output.bc") {
                println!("  → Wrote output.bc");
            }

            println!("\n── Next Steps ──");
            println!("  Compile to native:   llc output.ll -o output.s");
            println!("  Compile to object:   llc output.ll -filetype=obj -o output.o");
            println!("  Cross-compile ARM:   llc output.ll -mtriple=aarch64-linux-gnu -o output_arm.s");
        }
        Err(e) => {
            println!("Stage 4 — LLVM IR Generation");
            println!("  ✗ {}\n", e);
        }
    }
}