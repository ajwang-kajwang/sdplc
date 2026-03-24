(* all_control_flow.st — Every control flow construct
   Testing: IF, FOR, WHILE, REPEAT, CASE, EXIT, RETURN
   Compile: sdplc examples/all_control_flow.st
*)

FUNCTION Factorial : DINT
VAR_INPUT
    n : DINT;
END_VAR
VAR
    result : DINT := 1;
    i : DINT;
END_VAR

IF n <= 1 THEN
    Factorial := 1;
    RETURN;
END_IF;

FOR i := 2 TO n DO
    result := result * i;
END_FOR;

Factorial := result;

END_FUNCTION


PROGRAM ControlFlowDemo
VAR
    i : INT;
    j : INT;
    sum : DINT := 0;
    found : BOOL := FALSE;
    state : INT := 0;
    output : REAL := 0.0;
    limit : INT := 50;
END_VAR

(* ── FOR with BY step ── *)
FOR i := 0 TO 100 BY 5 DO
    sum := sum + i;
    IF sum > 500 THEN
        EXIT;
    END_IF;
END_FOR;

(* ── Nested FOR with EXIT ── *)
FOR i := 0 TO 9 DO
    FOR j := 0 TO 9 DO
        IF i * 10 + j >= limit THEN
            found := TRUE;
            EXIT;
        END_IF;
    END_FOR;
    IF found THEN
        EXIT;
    END_IF;
END_FOR;

(* ── WHILE ── *)
WHILE sum > 100 DO
    sum := sum / 2;
END_WHILE;

(* ── REPEAT / UNTIL ── *)
REPEAT
    sum := sum + 3;
UNTIL sum >= 50
END_REPEAT;

(* ── CASE with ranges and ELSE ── *)
CASE state OF
    0:
        output := 0.0;
    1..3:
        output := 25.0;
    4, 5:
        output := 50.0;
    6..9:
        output := 75.0;
ELSE
    output := 100.0;
END_CASE;

(* ── Deeply nested IF / ELSIF / ELSE ── *)
IF state = 0 THEN
    IF found THEN
        output := -1.0;
    ELSE
        output := -2.0;
    END_IF;
ELSIF state = 1 THEN
    output := 10.0;
ELSIF state = 2 THEN
    output := 20.0;
ELSE
    output := 99.0;
END_IF;

END_PROGRAM