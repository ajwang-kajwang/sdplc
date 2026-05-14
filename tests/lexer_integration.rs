//! Integration tests for the SD-PLC lexer.
//!
//! These tests exercise the lexer against complete, realistic IEC 61131-3
//! programs to verify that the full pipeline continues to work correctly
//! as the codebase evolves (parser, codegen, etc.).

use sdplc::lexer::{Lexer, TokenType};

// ─── Full Program Tests ─────────────────────────────────────────

#[test]
fn test_conveyor_control_program() {
    let source = r#"
PROGRAM ConveyorControl
VAR
    speed : REAL := 0.0;
    running : BOOL := FALSE;
END_VAR
IF running AND speed > 0.0 THEN
    running := FALSE; // safety stop
END_IF;
END_PROGRAM
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    // No unknown tokens
    let unknown: Vec<_> = tokens
        .iter()
        .filter(|t| t.kind == TokenType::Unknown)
        .collect();
    assert!(
        unknown.is_empty(),
        "Lexer produced {} unknown token(s): {:?}",
        unknown.len(),
        unknown.iter().map(|t| &t.text).collect::<Vec<_>>()
    );

    // Correct bookends
    assert_eq!(tokens.first().unwrap().kind, TokenType::Program);
    assert_eq!(tokens[tokens.len() - 2].kind, TokenType::EndProgram);
    assert_eq!(tokens.last().unwrap().kind, TokenType::Eof);
}

#[test]
fn test_function_block_with_io() {
    let source = r#"
FUNCTION_BLOCK PID_Controller
VAR_INPUT
    setpoint : REAL;
    process_var : REAL;
END_VAR
VAR_OUTPUT
    output : REAL;
END_VAR
VAR
    error : REAL;
    integral : REAL := 0.0;
    kp : REAL := 1.0;
    ki : REAL := 0.1;
END_VAR

error := setpoint - process_var;
integral := integral + error;
output := kp * error + ki * integral;

END_FUNCTION_BLOCK
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let unknown_count = tokens
        .iter()
        .filter(|t| t.kind == TokenType::Unknown)
        .count();
    assert_eq!(unknown_count, 0, "Unexpected unknown tokens in FB program");

    // Verify key structural tokens are present
    let kinds: Vec<TokenType> = tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&TokenType::FunctionBlock));
    assert!(kinds.contains(&TokenType::VarInput));
    assert!(kinds.contains(&TokenType::VarOutput));
    assert!(kinds.contains(&TokenType::EndFunctionBlock));
}

#[test]
fn test_all_loop_constructs() {
    let source = r#"
PROGRAM LoopDemo
VAR
    i : INT;
    sum : DINT := 0;
    flag : BOOL := TRUE;
END_VAR

(* FOR loop *)
FOR i := 1 TO 100 BY 2 DO
    sum := sum + i;
END_FOR;

(* WHILE loop *)
WHILE sum > 1000 DO
    sum := sum / 2;
END_WHILE;

(* REPEAT loop *)
REPEAT
    sum := sum + 1;
UNTIL sum >= 500
END_REPEAT;

END_PROGRAM
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let unknown_count = tokens
        .iter()
        .filter(|t| t.kind == TokenType::Unknown)
        .count();
    assert_eq!(unknown_count, 0);

    let kinds: Vec<TokenType> = tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&TokenType::For));
    assert!(kinds.contains(&TokenType::EndFor));
    assert!(kinds.contains(&TokenType::While));
    assert!(kinds.contains(&TokenType::EndWhile));
    assert!(kinds.contains(&TokenType::Repeat));
    assert!(kinds.contains(&TokenType::Until));
    assert!(kinds.contains(&TokenType::EndRepeat));
}

#[test]
fn test_case_with_ranges() {
    let source = r#"
PROGRAM CaseDemo
VAR
    state : INT := 0;
    output : REAL := 0.0;
END_VAR

CASE state OF
    0:
        output := 0.0;
    1..3:
        output := 25.0;
    4, 5:
        output := 50.0;
    6..10:
        output := 100.0;
ELSE
    output := -1.0;
END_CASE;

END_PROGRAM
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let unknown_count = tokens
        .iter()
        .filter(|t| t.kind == TokenType::Unknown)
        .count();
    assert_eq!(unknown_count, 0);

    // Verify range and comma tokens appear
    let kinds: Vec<TokenType> = tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&TokenType::Case));
    assert!(kinds.contains(&TokenType::DotDot));
    assert!(kinds.contains(&TokenType::Comma));
    assert!(kinds.contains(&TokenType::Else));
    assert!(kinds.contains(&TokenType::EndCase));
}

#[test]
fn test_array_and_struct_types() {
    let source = r#"
TYPE
    SensorData : STRUCT
        values : ARRAY[0..7] OF REAL;
        timestamp : TIME;
        valid : BOOL;
    END_STRUCT;
END_TYPE
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let unknown_count = tokens
        .iter()
        .filter(|t| t.kind == TokenType::Unknown)
        .count();
    assert_eq!(unknown_count, 0);

    let kinds: Vec<TokenType> = tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&TokenType::Type));
    assert!(kinds.contains(&TokenType::Struct));
    assert!(kinds.contains(&TokenType::Array));
    assert!(kinds.contains(&TokenType::LBracket));
    assert!(kinds.contains(&TokenType::DotDot));
    assert!(kinds.contains(&TokenType::RBracket));
    assert!(kinds.contains(&TokenType::Of));
    assert!(kinds.contains(&TokenType::EndStruct));
    assert!(kinds.contains(&TokenType::EndType));
}

#[test]
fn test_temporal_literals_in_context() {
    let source = r#"
PROGRAM TimerDemo
VAR
    cycle_time : TIME := T#100ms;
    start_date : DATE := D#2025-12-01;
    alarm_time : TOD := TOD#08:30:00;
    next_maint : DT := DT#2026-06-15-09:00:00;
END_VAR
END_PROGRAM
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    let unknown_count = tokens
        .iter()
        .filter(|t| t.kind == TokenType::Unknown)
        .count();
    assert_eq!(unknown_count, 0);

    let time_tokens: Vec<_> = tokens
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                TokenType::TimeLiteral
                    | TokenType::DateLiteral
                    | TokenType::TodLiteral
                    | TokenType::DtLiteral
            )
        })
        .collect();

    assert_eq!(time_tokens.len(), 4, "Expected 4 temporal literals");
    assert_eq!(time_tokens[0].kind, TokenType::TimeLiteral);
    assert_eq!(time_tokens[1].kind, TokenType::DateLiteral);
    assert_eq!(time_tokens[2].kind, TokenType::TodLiteral);
    assert_eq!(time_tokens[3].kind, TokenType::DtLiteral);
}

#[test]
fn test_mixed_comments_survive() {
    let source = r#"
PROGRAM CommentTest
(* block comment *)
VAR
    x : INT; // line comment
    (* nested (* comment *) here *)
    y : BOOL;
END_VAR
END_PROGRAM
"#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    // Comments should be stripped — only code tokens remain
    let unknown_count = tokens
        .iter()
        .filter(|t| t.kind == TokenType::Unknown)
        .count();
    assert_eq!(unknown_count, 0);

    // x and y should both be present as identifiers
    let idents: Vec<_> = tokens
        .iter()
        .filter(|t| t.kind == TokenType::Ident)
        .map(|t| t.text.as_str())
        .collect();
    assert!(idents.contains(&"x"));
    assert!(idents.contains(&"y"));
}

// ─── Edge Cases ─────────────────────────────────────────────────

#[test]
fn test_empty_input() {
    let mut lexer = Lexer::new("");
    let tokens = lexer.tokenize();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenType::Eof);
}

#[test]
fn test_whitespace_only() {
    let mut lexer = Lexer::new("   \n\t\n   ");
    let tokens = lexer.tokenize();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenType::Eof);
}

#[test]
fn test_token_count_stability() {
    // Regression guard: if lexer changes produce more or fewer tokens
    // for this fixed input, something has changed that needs review.
    let source = "PROGRAM P VAR x : INT := 0; END_VAR END_PROGRAM";
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    assert_eq!(
        tokens.len(),
        12,
        "Token count changed — review lexer changes"
    );
}
