(*
    Standard function block library demonstration.

    A small conveyor start/stop sequence that exercises every block in
    the SD-PLC standard library: the three timers, the three counters,
    both edge detectors and both latches. Run it with:

        cargo run --bin runtime -- programs/fb_library_demo.st --scan-time=50
*)
PROGRAM FbLibraryDemo
VAR
    (* Simulated field inputs, driven by the scan counter below so the
       program is self-exciting and needs no wiring to observe. *)
    scan_count : DINT := 0;
    start_button : BOOL := FALSE;
    stop_button : BOOL := FALSE;
    part_sensor : BOOL := FALSE;

    (* Function block instances — each one carries its own state. *)
    run_latch : RS;
    start_edge : R_TRIG;
    stop_edge : F_TRIG;
    warmup : TON;
    coast : TOF;
    horn : TP;
    parts : CTU;
    remaining : CTD;
    balance : CTUD;

    (* Outputs *)
    motor : BOOL := FALSE;
    conveyor_ready : BOOL := FALSE;
    brake : BOOL := FALSE;
    horn_on : BOOL := FALSE;
    warmup_elapsed : TIME;
    parts_done : BOOL := FALSE;
    parts_count : INT := 0;
    batch_empty : BOOL := FALSE;
    batch_reset : BOOL := FALSE;
END_VAR

scan_count := scan_count + 1;

(* Drive the simulated inputs: press start on scan 5, press and release
   stop around scan 200, and pulse the part sensor every 10 scans in
   between. Stop is released again so the horn's falling edge fires. *)
start_button := scan_count >= 5 AND scan_count < 10;
stop_button := scan_count >= 200 AND scan_count < 205;
part_sensor := (scan_count MOD 10) < 3 AND scan_count > 10 AND scan_count < 200;

(* Edge detection on the pushbuttons. *)
start_edge(CLK := start_button);
stop_edge(CLK := stop_button);

(* Reset-dominant latch: stop wins over start. *)
run_latch(S := start_edge.Q, R1 := stop_button);
motor := run_latch.Q1;

(* On-delay: the conveyor is only "ready" after the motor has run for
   two seconds continuously. *)
warmup(IN := motor, PT := T#2s);
conveyor_ready := warmup.Q;
warmup_elapsed := warmup.ET;

(* Off-delay: the brake stays engaged for 1.5s after the motor stops. *)
coast(IN := NOT motor, PT := T#1500ms);
brake := coast.Q;

(* Pulse: one 500ms horn blast on each falling edge of the stop button. *)
horn(IN := stop_edge.Q, PT := T#500ms);
horn_on := horn.Q;

(* Up counter: parts seen, Q latches once the batch of 12 is complete. *)
parts(CU := part_sensor, R := start_edge.Q, PV := 12);
parts_count := parts.CV;
parts_done := parts.Q;

(* Down counter: how many are left in the batch. *)
remaining(CD := part_sensor, LD := start_edge.Q, PV := 12);
batch_empty := remaining.Q;

(* Up/down counter: net balance of parts in against parts out. *)
balance(CU := part_sensor, CD := stop_button, R := start_edge.Q, PV := 12);
batch_reset := balance.QD;

END_PROGRAM
