//! Integration tests for SD-PLC LLVM IR code generation.
//!
//! These tests run the full lexer → parser → codegen pipeline and
//! verify the emitted LLVM IR contains expected instructions and
//! structure.

use inkwell::context::Context;
use sdplc::codegen::CodeGenerator;
use sdplc::lexer::Lexer;
use sdplc::parser::Parser;

fn compile_to_ir(src: &str) -> String {
    let lexer = Lexer::new(src);
    let mut parser = Parser::new(lexer);
    let ast = parser
        .parse()
        .unwrap_or_else(|e| panic!("Parse error: {}", e));

    let context = Context::create();
    let mut codegen = CodeGenerator::new(&context, "test");
    codegen
        .compile(&ast)
        .unwrap_or_else(|e| panic!("Codegen error: {}", e));
    codegen.ir_string()
}

// ─── Full Programs ──────────────────────────────────────────────

#[test]
fn test_conveyor_control_full_pipeline() {
    let ir = compile_to_ir(
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

CASE count MOD 4 OF
    0: speed := 25.0;
    1..2: speed := 50.0;
    3: speed := 75.0;
END_CASE;

END_PROGRAM
"#,
    );

    // Function signature
    assert!(
        ir.contains("define void @ConveyorControl()"),
        "Missing function declaration"
    );

    // Variable allocations
    assert!(ir.contains("alloca"), "Missing alloca instructions");

    // Control flow blocks
    assert!(ir.contains("if.then"), "Missing IF block");
    assert!(ir.contains("for.cond"), "Missing FOR condition block");
    assert!(ir.contains("for.body"), "Missing FOR body block");
    assert!(ir.contains("for.inc"), "Missing FOR increment block");

    // Array access
    assert!(ir.contains("getelementptr"), "Missing array GEP");

    // CASE
    assert!(ir.contains("case.test"), "Missing CASE test block");

    // Function terminates
    assert!(ir.contains("ret void"), "Missing return");
}

#[test]
fn test_function_with_return() {
    let ir = compile_to_ir(
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

    // Function with return type
    assert!(
        ir.contains("define i16 @Clamp(i16"),
        "Missing typed function"
    );
    assert!(ir.contains("ret i16"), "Missing typed return");
}

#[test]
fn test_pid_controller_fb() {
    let ir = compile_to_ir(
        r#"
FUNCTION_BLOCK PID
VAR_INPUT
    sp : REAL;
    pv : REAL;
    kp : REAL := 1.0;
END_VAR
VAR_OUTPUT
    out : REAL;
END_VAR
VAR
    err : REAL;
END_VAR

err := sp - pv;
out := kp * err;

END_FUNCTION_BLOCK
"#,
    );

    assert!(ir.contains("define void @PID()"), "Missing FB function");
    assert!(ir.contains("fsub"), "Missing float subtraction");
    assert!(ir.contains("fmul"), "Missing float multiplication");
}

#[test]
fn test_while_loop() {
    let ir = compile_to_ir(
        r#"
PROGRAM P
VAR x : INT := 100; END_VAR
WHILE x > 0 DO
    x := x - 1;
END_WHILE;
END_PROGRAM
"#,
    );

    assert!(ir.contains("while.cond"), "Missing WHILE cond block");
    assert!(ir.contains("while.body"), "Missing WHILE body block");
    assert!(ir.contains("while.exit"), "Missing WHILE exit block");
    assert!(ir.contains("icmp sgt"), "Missing signed comparison");
}

#[test]
fn test_repeat_loop() {
    let ir = compile_to_ir(
        r#"
PROGRAM P
VAR x : INT := 0; END_VAR
REPEAT
    x := x + 1;
UNTIL x >= 10
END_REPEAT;
END_PROGRAM
"#,
    );

    assert!(ir.contains("repeat.body"), "Missing REPEAT body block");
    assert!(ir.contains("repeat.cond"), "Missing REPEAT cond block");
}

#[test]
fn test_multiple_pous() {
    let ir = compile_to_ir(
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

    assert!(
        ir.contains("define i32 @Square(i32"),
        "Missing Square function"
    );
    assert!(ir.contains("define void @Main()"), "Missing Main program");
}

#[test]
fn test_boolean_logic() {
    let ir = compile_to_ir(
        r#"
PROGRAM P
VAR a : BOOL; b : BOOL; c : BOOL; END_VAR
c := a AND b;
c := a OR b;
c := a XOR b;
c := NOT a;
END_PROGRAM
"#,
    );

    assert!(ir.contains(" and i1"), "Missing AND");
    assert!(ir.contains(" or i1"), "Missing OR");
    assert!(ir.contains(" xor i1"), "Missing XOR");
}

#[test]
fn test_mixed_numeric_operations() {
    let ir = compile_to_ir(
        r#"
PROGRAM P
VAR
    x : INT := 10;
    y : REAL := 3.14;
    flag : BOOL;
END_VAR
y := y + 1.0;
flag := x > 5;
END_PROGRAM
"#,
    );

    assert!(ir.contains("fadd"), "Missing float add");
    assert!(ir.contains("icmp sgt"), "Missing int comparison");
}

#[test]
fn test_nested_if_elsif_else() {
    let ir = compile_to_ir(
        r#"
PROGRAM P
VAR x : INT := 0; END_VAR
IF x > 10 THEN
    x := 10;
ELSIF x > 5 THEN
    x := 5;
ELSIF x > 0 THEN
    x := 1;
ELSE
    x := 0;
END_IF;
END_PROGRAM
"#,
    );

    assert!(ir.contains("if.then"), "Missing then block");
    assert!(ir.contains("elsif.then"), "Missing elsif block");
    // Should have multiple conditional branches
    let br_count = ir.matches("br i1").count();
    assert!(
        br_count >= 3,
        "Expected at least 3 conditional branches, found {}",
        br_count
    );
}

#[test]
fn test_array_operations() {
    let ir = compile_to_ir(
        r#"
PROGRAM P
VAR
    data : ARRAY[0..9] OF DINT;
    i : INT;
END_VAR
FOR i := 0 TO 9 DO
    data[i] := i;
END_FOR;
END_PROGRAM
"#,
    );

    assert!(ir.contains("getelementptr"), "Missing GEP for array access");
    assert!(ir.contains("for.cond"), "Missing FOR loop");
}

#[test]
fn test_exponentiation() {
    let ir = compile_to_ir(
        r#"
PROGRAM P
VAR x : REAL := 2.0; END_VAR
x := x ** 3.0;
END_PROGRAM
"#,
    );

    assert!(
        ir.contains("llvm.pow.f64") || ir.contains("pow"),
        "Missing power intrinsic"
    );
}
