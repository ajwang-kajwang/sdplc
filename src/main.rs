mod lexer;
use lexer::{Lexer, TokenType};

fn main() {
    let source_code = r#"
PROGRAM ConveyorControl
VAR
    speed : REAL := 0.0;
    running : BOOL := FALSE;
    count : INT := 0;
    limit : INT := 1_000;
    sensor_vals : ARRAY[0..7] OF DINT;
    timeout : TIME := T#5s;
END_VAR

(* Main conveyor control logic *)
IF running AND speed > 0.0 THEN
    count := count + 1;
    
    // Safety: stop if we exceed the limit
    IF count >= limit THEN
        running := FALSE;
        speed := 0.0;
    ELSIF speed > 100.0 THEN
        speed := 100.0;  (* clamp to max *)
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

    println!("═══ SD-PLC Lexer ═══\n");
    println!("Scanning IEC 61131-3 Structured Text ({} chars)\n", source_code.len());

    let mut lexer = Lexer::new(source_code);
    let tokens = lexer.tokenize();

    // Print token table
    println!("{:<6} {:<4} {:<22} {}", "Line", "Col", "Token Type", "Text");
    println!("{}", "─".repeat(60));

    let mut unknown_count = 0;
    for token in &tokens {
        if token.kind == TokenType::Unknown {
            unknown_count += 1;
        }
        println!(
            "{:<6} {:<4} {:<22} '{}'",
            token.line, token.col, format!("{:?}", token.kind), token.text
        );
    }

    println!("\n{}", "─".repeat(60));
    println!("Total tokens: {}", tokens.len());
    println!("Unknown tokens: {}", unknown_count);

    if unknown_count == 0 {
        println!("\n✓ All tokens recognised successfully.");
    } else {
        println!("\n✗ {} unrecognised token(s) — check input.", unknown_count);
    }
}