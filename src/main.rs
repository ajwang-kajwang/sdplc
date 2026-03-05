use sdplc::ast::*;
use sdplc::lexer::Lexer;
use sdplc::parser::Parser;
use sdplc::semantic;

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

    println!("═══ SD-PLC Compiler Frontend ═══\n");

    // ── Stage 1: Lexing ──
    let mut token_lexer = Lexer::new(source_code);
    let tokens = token_lexer.tokenize();
    let unknown_count = tokens.iter()
        .filter(|t| t.kind == sdplc::lexer::TokenType::Unknown)
        .count();

    println!("Stage 1 — Lexer");
    println!("  Source: {} chars → {} tokens", source_code.len(), tokens.len());
    if unknown_count > 0 {
        println!("  ✗ {} unknown token(s) — aborting.", unknown_count);
        return;
    }
    println!("  ✓ All tokens recognised.\n");

    // ── Stage 2: Parsing ──
    let lexer = Lexer::new(source_code);
    let mut parser = Parser::new(lexer);
    let ast = match parser.parse() {
        Ok(ast) => {
            println!("Stage 2 — Parser");
            println!("  ✓ AST built ({} POU(s)).\n", ast.units.len());
            ast
        }
        Err(e) => {
            println!("Stage 2 — Parser");
            println!("  ✗ {}\n", e);
            return;
        }
    };

    // ── Stage 3: Semantic Analysis ──
    let ctx = semantic::analyze(ast);

    println!("Stage 3 — Semantic Analysis");
    println!("  Errors:   {}", ctx.error_count());
    println!("  Warnings: {}", ctx.warning_count());

    for d in &ctx.diagnostics {
        println!("  {}", d);
    }

    if ctx.has_errors() {
        println!("  ✗ Compilation aborted.\n");
        return;
    }
    println!("  ✓ All checks passed.\n");

    // ── Summary ──
    println!("── AST Summary ──\n");
    for pou in &ctx.ast.units {
        match pou {
            Pou::Program(p) => {
                println!("  PROGRAM {}", p.name);
                for vb in &p.var_blocks {
                    for decl in &vb.declarations {
                        let sym = ctx.symbols.lookup(&decl.name);
                        let resolved = sym
                            .map(|s| format!("{}", s.resolved_type))
                            .unwrap_or_else(|| "?".to_string());
                        println!("    {} : {} → {}", decl.name, format!("{:?}", decl.type_spec), resolved);
                    }
                }
                println!("    Body: {} statement(s)", p.body.len());
                for (i, stmt) in p.body.iter().enumerate() {
                    println!("      [{}] {}", i, describe_stmt(stmt));
                }
            }
            Pou::Function(f) => println!("  FUNCTION {}", f.name),
            Pou::FunctionBlock(fb) => println!("  FUNCTION_BLOCK {}", fb.name),
        }
    }
    println!("\n✓ Ready for LLVM IR generation.");
}

fn describe_stmt(stmt: &Statement) -> String {
    match stmt {
        Statement::Assignment { .. } => "Assignment".to_string(),
        Statement::If { elsif_branches, else_body, .. } => format!(
            "IF ({} elsif, {})",
            elsif_branches.len(),
            if else_body.is_some() { "else" } else { "no else" }
        ),
        Statement::For { variable, .. } => format!("FOR {}", variable),
        Statement::While { .. } => "WHILE".to_string(),
        Statement::Repeat { .. } => "REPEAT".to_string(),
        Statement::Case { branches, .. } => format!("CASE ({} branches)", branches.len()),
        Statement::Exit { .. } => "EXIT".to_string(),
        Statement::Return { .. } => "RETURN".to_string(),
        Statement::CallStatement { name, .. } => format!("CALL {}", name),
        Statement::Empty => ";".to_string(),
    }
}