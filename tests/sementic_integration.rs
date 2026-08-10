//! Integration tests for SD-PLC semantic analysis.
//!
//! These tests run the full lexer → parser → semantic pipeline
//! against complete programs, verifying both valid and invalid cases.

use sdplc::lexer::Lexer;
use sdplc::parser::Parser;
use sdplc::semantic::{self, ProgramContext, Severity};

fn run_analysis(src: &str) -> ProgramContext {
    let lexer = Lexer::new(src);
    let mut parser = Parser::new(lexer);
    let ast = parser
        .parse()
        .unwrap_or_else(|e| panic!("Parse error: {}", e));
    semantic::analyze(ast)
}

fn assert_clean(src: &str) -> ProgramContext {
    let ctx = run_analysis(src);
    if ctx.has_errors() {
        for d in &ctx.diagnostics {
            eprintln!("  {}", d);
        }
        panic!("Expected no errors, found {}", ctx.error_count());
    }
    ctx
}

fn assert_errors(src: &str) -> ProgramContext {
    let ctx = run_analysis(src);
    assert!(ctx.has_errors(), "Expected semantic errors, found none");
    ctx
}

// ─── Valid Programs ─────────────────────────────────────────────

#[test]
fn test_conveyor_control_passes() {
    assert_clean(
        r#"
PROGRAM ConveyorControl
VAR
    speed : REAL := 0.0;
    running : BOOL := FALSE;
    count : INT := 0;
    limit : INT := 1000;
    i : INT;
    sensor_vals : ARRAY[0..7] OF DINT;
END_VAR

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

END_PROGRAM
"#,
    );
}

#[test]
fn test_pid_controller_passes() {
    assert_clean(
        r#"
FUNCTION_BLOCK PID_Controller
VAR_INPUT
    setpoint : REAL;
    process_var : REAL;
    kp : REAL := 1.0;
    ki : REAL := 0.1;
END_VAR
VAR_OUTPUT
    output : REAL;
END_VAR
VAR
    error : REAL;
    integral : REAL := 0.0;
END_VAR

error := setpoint - process_var;
integral := integral + error;
output := kp * error + ki * integral;

END_FUNCTION_BLOCK
"#,
    );
}

#[test]
fn test_function_with_return_passes() {
    assert_clean(
        r#"
FUNCTION Clamp : INT
VAR_INPUT
    value : INT;
    low : INT;
    high : INT;
END_VAR

IF value < low THEN
    Clamp := low;
ELSIF value > high THEN
    Clamp := high;
ELSE
    Clamp := value;
END_IF;

END_FUNCTION
"#,
    );
}

#[test]
fn test_all_loop_types_pass() {
    assert_clean(
        r#"
PROGRAM Loops
VAR
    i : INT;
    sum : DINT := 0;
    flag : BOOL := FALSE;
END_VAR

FOR i := 1 TO 100 BY 2 DO
    sum := sum + i;
END_FOR;

WHILE sum > 1000 DO
    sum := sum / 2;
END_WHILE;

REPEAT
    sum := sum + 1;
UNTIL sum >= 500
END_REPEAT;

END_PROGRAM
"#,
    );
}

#[test]
fn test_nested_loops_with_exit() {
    assert_clean(
        r#"
PROGRAM Nested
VAR
    i : INT;
    j : INT;
    found : BOOL := FALSE;
END_VAR

FOR i := 0 TO 9 DO
    FOR j := 0 TO 9 DO
        IF i + j > 15 THEN
            found := TRUE;
            EXIT;
        END_IF;
    END_FOR;
END_FOR;

END_PROGRAM
"#,
    );
}

#[test]
fn test_multiple_pous_pass() {
    assert_clean(
        r#"
FUNCTION Square : DINT
VAR_INPUT n : DINT; END_VAR
Square := n * n;
END_FUNCTION

PROGRAM Main
VAR x : DINT := 5; END_VAR
x := x + 1;
END_PROGRAM
"#,
    );
}

// ─── Symbol Table Queries ───────────────────────────────────────

#[test]
fn test_symbol_table_populated() {
    let ctx = assert_clean("PROGRAM P VAR x : INT; y : REAL; z : BOOL; END_VAR END_PROGRAM");

    // After analysis, the POU scope has been popped, but we can
    // verify diagnostics are empty (types were resolved correctly).
    assert_eq!(ctx.error_count(), 0);
    assert_eq!(ctx.warning_count(), 0);
}

// ─── Type Errors ────────────────────────────────────────────────

#[test]
fn test_bool_assigned_to_int() {
    let ctx = assert_errors("PROGRAM P VAR x : INT; END_VAR x := TRUE; END_PROGRAM");
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("cannot assign"))
    );
}

#[test]
fn test_string_in_arithmetic() {
    let ctx = assert_errors("PROGRAM P VAR s : STRING; x : INT; END_VAR x := x + s; END_PROGRAM");
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("requires numeric"))
    );
}

#[test]
fn test_non_bool_if_condition() {
    let ctx = assert_errors("PROGRAM P VAR x : INT; END_VAR IF x THEN x := 0; END_IF; END_PROGRAM");
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("must be BOOL"))
    );
}

#[test]
fn test_float_for_variable() {
    let ctx = assert_errors(
        "PROGRAM P VAR r : REAL; END_VAR \
         FOR r := 0.0 TO 10.0 DO r := r + 1.0; END_FOR; END_PROGRAM",
    );
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("must be an integer type"))
    );
}

#[test]
fn test_exit_outside_loop() {
    let ctx = assert_errors("PROGRAM P VAR x : INT; END_VAR EXIT; END_PROGRAM");
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("EXIT is only valid inside a loop"))
    );
}

#[test]
fn test_case_with_float_selector() {
    let ctx = assert_errors(
        "PROGRAM P VAR r : REAL; END_VAR \
         CASE r OF 0: r := 1.0; END_CASE; END_PROGRAM",
    );
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("CASE selector must be integer"))
    );
}

#[test]
fn test_assign_to_constant() {
    let ctx = assert_errors(
        "PROGRAM P \
         VAR CONSTANT MAX : INT := 100; END_VAR \
         MAX := 200; END_PROGRAM",
    );
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("CONSTANT"))
    );
}

#[test]
fn test_array_float_index() {
    let ctx = assert_errors(
        "PROGRAM P VAR a : ARRAY[0..9] OF INT; r : REAL; END_VAR \
         a[r] := 0; END_PROGRAM",
    );
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("array index must be integer"))
    );
}

#[test]
fn test_subscript_on_non_array() {
    let ctx = assert_errors("PROGRAM P VAR x : INT; END_VAR x[0] := 1; END_PROGRAM");
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("non-array type"))
    );
}

#[test]
fn test_mod_with_floats() {
    let ctx = assert_errors("PROGRAM P VAR x : REAL; END_VAR x := x MOD 2.0; END_PROGRAM");
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("MOD requires integer"))
    );
}

#[test]
fn test_duplicate_declaration() {
    let ctx = assert_errors("PROGRAM P VAR x : INT; x : REAL; END_VAR END_PROGRAM");
    assert!(
        ctx.diagnostics
            .iter()
            .any(|d| d.message.contains("already declared"))
    );
}
