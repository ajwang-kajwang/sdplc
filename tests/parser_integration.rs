//! Integration tests for the SD-PLC parser.
//!
//! These tests exercise the full lexer → parser pipeline against
//! complete IEC 61131-3 programs.

use sdplc::ast::*;
use sdplc::lexer::Lexer;
use sdplc::parser::Parser;

fn parse_ok(src: &str) -> CompilationUnit {
    let lexer = Lexer::new(src);
    let mut parser = Parser::new(lexer);
    parser.parse().unwrap_or_else(|e| panic!("Parse error: {}", e))
}

#[test]
fn test_pid_controller_function_block() {
    let src = r#"
FUNCTION_BLOCK PID_Controller
VAR_INPUT
    setpoint : REAL;
    process_var : REAL;
    kp : REAL := 1.0;
    ki : REAL := 0.1;
    kd : REAL := 0.01;
END_VAR
VAR_OUTPUT
    output : REAL;
END_VAR
VAR
    error : REAL;
    prev_error : REAL := 0.0;
    integral : REAL := 0.0;
    derivative : REAL;
END_VAR

error := setpoint - process_var;
integral := integral + error;
derivative := error - prev_error;
output := kp * error + ki * integral + kd * derivative;
prev_error := error;

END_FUNCTION_BLOCK
"#;

    let unit = parse_ok(src);
    assert_eq!(unit.units.len(), 1);
    if let Pou::FunctionBlock(fb) = &unit.units[0] {
        assert_eq!(fb.name, "PID_Controller");
        assert_eq!(fb.var_blocks.len(), 3);
        assert_eq!(fb.var_blocks[0].qualifier, VarQualifier::VarInput);
        assert_eq!(fb.var_blocks[0].declarations.len(), 5);
        assert_eq!(fb.var_blocks[1].qualifier, VarQualifier::VarOutput);
        assert_eq!(fb.var_blocks[2].qualifier, VarQualifier::Var);
        assert_eq!(fb.body.len(), 5); // 5 assignments
    } else {
        panic!("expected FunctionBlock");
    }
}

#[test]
fn test_abs_function() {
    let src = r#"
FUNCTION MyAbs : INT
VAR_INPUT
    x : INT;
END_VAR

IF x < 0 THEN
    MyAbs := -x;
ELSE
    MyAbs := x;
END_IF;

END_FUNCTION
"#;

    let unit = parse_ok(src);
    if let Pou::Function(f) = &unit.units[0] {
        assert_eq!(f.name, "MyAbs");
        assert_eq!(f.return_type, TypeSpec::Elementary(ElementaryType::Int));
        assert_eq!(f.body.len(), 1); // one IF statement
    } else {
        panic!("expected Function");
    }
}

#[test]
fn test_multiple_pous() {
    let src = r#"
FUNCTION Square : DINT
VAR_INPUT n : DINT; END_VAR
Square := n * n;
END_FUNCTION

FUNCTION_BLOCK Accumulator
VAR_INPUT value : DINT; END_VAR
VAR_OUTPUT total : DINT; END_VAR
VAR sum : DINT := 0; END_VAR
sum := sum + value;
total := sum;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    x : DINT := 5;
END_VAR
x := x + 1;
END_PROGRAM
"#;

    let unit = parse_ok(src);
    assert_eq!(unit.units.len(), 3);
    assert!(matches!(&unit.units[0], Pou::Function(_)));
    assert!(matches!(&unit.units[1], Pou::FunctionBlock(_)));
    assert!(matches!(&unit.units[2], Pou::Program(_)));
}

#[test]
fn test_conveyor_control_full() {
    let src = r#"
PROGRAM ConveyorControl
VAR
    speed : REAL := 0.0;
    running : BOOL := FALSE;
    count : INT := 0;
    limit : INT := 1000;
    sensor_vals : ARRAY[0..7] OF DINT;
END_VAR

(* Main control logic *)
IF running AND speed > 0.0 THEN
    count := count + 1;
    IF count >= limit THEN
        running := FALSE;
        speed := 0.0;
    ELSIF speed > 100.0 THEN
        speed := 100.0;
    END_IF;
END_IF;

// Sensor scan loop
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

    let unit = parse_ok(src);
    if let Pou::Program(p) = &unit.units[0] {
        assert_eq!(p.name, "ConveyorControl");
        assert_eq!(p.var_blocks[0].declarations.len(), 5);
        assert_eq!(p.body.len(), 3); // IF, FOR, CASE

        // Verify the FOR loop has array access
        if let Statement::For { body, .. } = &p.body[1] {
            if let Statement::Assignment { target, .. } = &body[0] {
                assert!(matches!(target, Expression::ArrayAccess { .. }));
            }
        }
    }
}

#[test]
fn test_nested_loops() {
    let src = r#"
PROGRAM Nested
VAR
    i : INT;
    j : INT;
    done : BOOL := FALSE;
END_VAR

FOR i := 0 TO 9 DO
    FOR j := 0 TO 9 DO
        IF i * 10 + j > 50 THEN
            done := TRUE;
            EXIT;
        END_IF;
    END_FOR;
    IF done THEN
        EXIT;
    END_IF;
END_FOR;

END_PROGRAM
"#;

    let unit = parse_ok(src);
    if let Pou::Program(p) = &unit.units[0] {
        assert_eq!(p.body.len(), 1); // outer FOR
        if let Statement::For { body, .. } = &p.body[0] {
            assert_eq!(body.len(), 2); // inner FOR + IF
        }
    }
}

#[test]
fn test_complex_expressions() {
    let src = r#"
PROGRAM ExprTest
VAR
    a : REAL;
    b : REAL;
    c : REAL;
    flag : BOOL;
END_VAR

a := (b + c) * 2.0 - 1.0;
flag := a > 0.0 AND a < 100.0 OR NOT flag;
b := a ** 2.0;

END_PROGRAM
"#;

    let unit = parse_ok(src);
    if let Pou::Program(p) = &unit.units[0] {
        assert_eq!(p.body.len(), 3);

        // Verify power operator parsed
        if let Statement::Assignment { value, .. } = &p.body[2] {
            if let Expression::BinaryOp { op, .. } = value {
                assert_eq!(*op, BinaryOperator::Power);
            } else {
                panic!("expected BinaryOp for **");
            }
        }
    }
}

#[test]
fn test_retain_constant_qualifiers() {
    let src = r#"
PROGRAM P
VAR RETAIN
    persistent_count : DINT := 0;
END_VAR
VAR CONSTANT
    MAX_SPEED : REAL := 100.0;
END_VAR
END_PROGRAM
"#;

    let unit = parse_ok(src);
    if let Pou::Program(p) = &unit.units[0] {
        assert!(p.var_blocks[0].retain);
        assert!(!p.var_blocks[0].constant);
        assert!(!p.var_blocks[1].retain);
        assert!(p.var_blocks[1].constant);
    }
}

#[test]
fn test_string_typed_vars() {
    let src = r#"
PROGRAM P
VAR
    msg : STRING;
    label : STRING;
END_VAR
END_PROGRAM
"#;

    let unit = parse_ok(src);
    if let Pou::Program(p) = &unit.units[0] {
        assert!(matches!(
            p.var_blocks[0].declarations[0].type_spec,
            TypeSpec::StringType { max_len: None }
        ));
    }
}