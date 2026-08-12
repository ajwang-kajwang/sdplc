(*
    SD-PLC Standard Function Block Library
    ======================================

    The IEC 61131-3 standard function blocks, written in Structured Text
    and compiled by SD-PLC's own pipeline — there is no hand-written LLVM
    IR here and no special case in codegen.rs for any individual block.

    Every block below is an ordinary FUNCTION_BLOCK: a struct of fields
    (VAR_INPUT, VAR_OUTPUT, VAR) plus a body. The compiler gives each
    declared instance its own copy of that struct, so ten TON instances
    hold ten independent timers.

    The only thing these bodies use that a user program cannot is the
    TIME_MS() intrinsic, which reads the runtime's scan-cycle clock
    (milliseconds, monotonic). See `Developer_Guide.md` §13.

    Semantics notes where the standard leaves room:
      - TIME values are milliseconds held in a 64-bit integer.
      - RS is reset-dominant, SR is set-dominant (IEC 61131-3 §6.6.2.4).
      - F_TRIG's internal M initialises TRUE so a program that starts with
        CLK = FALSE does not see a spurious falling edge on scan 1.
      - Counters saturate at the INT bounds rather than wrapping.
*)

(* ─── TON — on-delay timer ──────────────────────────────────────
   Q goes TRUE once IN has been continuously TRUE for PT.
   ET counts up to PT and holds there while IN stays TRUE. *)
FUNCTION_BLOCK TON
VAR_INPUT
    IN : BOOL;
    PT : TIME;
END_VAR
VAR_OUTPUT
    Q : BOOL;
    ET : TIME;
END_VAR
VAR
    start_time : TIME;
    running : BOOL;
END_VAR
    IF IN THEN
        IF NOT running THEN
            running := TRUE;
            start_time := TIME_MS();
        END_IF;
        ET := TIME_MS() - start_time;
        IF ET >= PT THEN
            ET := PT;
            Q := TRUE;
        ELSE
            Q := FALSE;
        END_IF;
    ELSE
        running := FALSE;
        Q := FALSE;
        ET := T#0ms;
    END_IF;
END_FUNCTION_BLOCK

(* ─── TOF — off-delay timer ─────────────────────────────────────
   Q follows IN up, then stays TRUE for PT after IN falls.
   ET runs only during that hold-over period. *)
FUNCTION_BLOCK TOF
VAR_INPUT
    IN : BOOL;
    PT : TIME;
END_VAR
VAR_OUTPUT
    Q : BOOL;
    ET : TIME;
END_VAR
VAR
    start_time : TIME;
    timing : BOOL;
    prev_in : BOOL;
END_VAR
    IF IN THEN
        Q := TRUE;
        ET := T#0ms;
        timing := FALSE;
    ELSE
        IF prev_in THEN
            timing := TRUE;
            start_time := TIME_MS();
        END_IF;
        IF timing THEN
            ET := TIME_MS() - start_time;
            IF ET >= PT THEN
                ET := PT;
                Q := FALSE;
                timing := FALSE;
            ELSE
                Q := TRUE;
            END_IF;
        END_IF;
    END_IF;
    prev_in := IN;
END_FUNCTION_BLOCK

(* ─── TP — pulse timer ──────────────────────────────────────────
   A rising edge on IN emits one pulse of exactly PT on Q.
   Further edges during the pulse are ignored (non-retriggerable). *)
FUNCTION_BLOCK TP
VAR_INPUT
    IN : BOOL;
    PT : TIME;
END_VAR
VAR_OUTPUT
    Q : BOOL;
    ET : TIME;
END_VAR
VAR
    start_time : TIME;
    running : BOOL;
    prev_in : BOOL;
END_VAR
    IF running THEN
        ET := TIME_MS() - start_time;
        IF ET >= PT THEN
            ET := PT;
            Q := FALSE;
            running := FALSE;
        ELSE
            Q := TRUE;
        END_IF;
    ELSE
        IF IN AND NOT prev_in THEN
            running := TRUE;
            start_time := TIME_MS();
            ET := T#0ms;
            Q := TRUE;
        ELSE
            Q := FALSE;
            IF NOT IN THEN
                ET := T#0ms;
            END_IF;
        END_IF;
    END_IF;
    prev_in := IN;
END_FUNCTION_BLOCK

(* ─── CTU — up counter ──────────────────────────────────────────
   CV increments on each rising edge of CU; R clears it.
   Q is TRUE while CV has reached the preset PV. *)
FUNCTION_BLOCK CTU
VAR_INPUT
    CU : BOOL;
    R : BOOL;
    PV : INT;
END_VAR
VAR_OUTPUT
    Q : BOOL;
    CV : INT;
END_VAR
VAR
    prev_cu : BOOL;
END_VAR
    IF R THEN
        CV := 0;
    ELSIF CU AND NOT prev_cu THEN
        IF CV < 32767 THEN
            CV := CV + 1;
        END_IF;
    END_IF;
    Q := CV >= PV;
    prev_cu := CU;
END_FUNCTION_BLOCK

(* ─── CTD — down counter ────────────────────────────────────────
   LD loads CV from PV; CV decrements on each rising edge of CD.
   Q is TRUE once CV has counted down to zero. *)
FUNCTION_BLOCK CTD
VAR_INPUT
    CD : BOOL;
    LD : BOOL;
    PV : INT;
END_VAR
VAR_OUTPUT
    Q : BOOL;
    CV : INT;
END_VAR
VAR
    prev_cd : BOOL;
END_VAR
    IF LD THEN
        CV := PV;
    ELSIF CD AND NOT prev_cd THEN
        IF CV > -32768 THEN
            CV := CV - 1;
        END_IF;
    END_IF;
    Q := CV <= 0;
    prev_cd := CD;
END_FUNCTION_BLOCK

(* ─── CTUD — up/down counter ────────────────────────────────────
   R (clear) wins over LD (load); simultaneous CU and CD edges cancel.
   QU tracks CV >= PV, QD tracks CV <= 0. *)
FUNCTION_BLOCK CTUD
VAR_INPUT
    CU : BOOL;
    CD : BOOL;
    R : BOOL;
    LD : BOOL;
    PV : INT;
END_VAR
VAR_OUTPUT
    QU : BOOL;
    QD : BOOL;
    CV : INT;
END_VAR
VAR
    prev_cu : BOOL;
    prev_cd : BOOL;
    up : BOOL;
    down : BOOL;
END_VAR
    up := CU AND NOT prev_cu;
    down := CD AND NOT prev_cd;
    IF R THEN
        CV := 0;
    ELSIF LD THEN
        CV := PV;
    ELSE
        IF up AND NOT down AND CV < 32767 THEN
            CV := CV + 1;
        END_IF;
        IF down AND NOT up AND CV > -32768 THEN
            CV := CV - 1;
        END_IF;
    END_IF;
    QU := CV >= PV;
    QD := CV <= 0;
    prev_cu := CU;
    prev_cd := CD;
END_FUNCTION_BLOCK

(* ─── R_TRIG — rising edge detector ─────────────────────────────
   Q is TRUE for exactly one scan after CLK goes FALSE → TRUE. *)
FUNCTION_BLOCK R_TRIG
VAR_INPUT
    CLK : BOOL;
END_VAR
VAR_OUTPUT
    Q : BOOL;
END_VAR
VAR
    M : BOOL;
END_VAR
    Q := CLK AND NOT M;
    M := CLK;
END_FUNCTION_BLOCK

(* ─── F_TRIG — falling edge detector ────────────────────────────
   Q is TRUE for exactly one scan after CLK goes TRUE → FALSE.
   M starts TRUE so scan 1 with CLK = FALSE is not an edge. *)
FUNCTION_BLOCK F_TRIG
VAR_INPUT
    CLK : BOOL;
END_VAR
VAR_OUTPUT
    Q : BOOL;
END_VAR
VAR
    M : BOOL := TRUE;
END_VAR
    Q := NOT CLK AND NOT M;
    M := NOT CLK;
END_FUNCTION_BLOCK

(* ─── RS — reset-dominant latch ─────────────────────────────────
   S sets Q1; R1 clears it and wins when both are TRUE. *)
FUNCTION_BLOCK RS
VAR_INPUT
    S : BOOL;
    R1 : BOOL;
END_VAR
VAR_OUTPUT
    Q1 : BOOL;
END_VAR
    Q1 := NOT R1 AND (S OR Q1);
END_FUNCTION_BLOCK

(* ─── SR — set-dominant latch ───────────────────────────────────
   S1 sets Q1 and wins when both inputs are TRUE; R clears it. *)
FUNCTION_BLOCK SR
VAR_INPUT
    S1 : BOOL;
    R : BOOL;
END_VAR
VAR_OUTPUT
    Q1 : BOOL;
END_VAR
    Q1 := S1 OR (NOT R AND Q1);
END_FUNCTION_BLOCK
