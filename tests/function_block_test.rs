//! Behavioural tests for the IEC 61131-3 standard function block library.
//!
//! Checking that the generated IR *contains* the right instructions only
//! proves the shape of the code. These tests JIT-execute the compiled
//! program instead and drive it scan by scan, so what is asserted is the
//! behaviour a plant would actually see: a TON that holds its output off
//! until the preset has genuinely elapsed, a counter that survives
//! between scan cycles, an edge detector that fires exactly once.
//!
//! The scan clock is driven explicitly rather than read from the wall
//! clock, which makes every timing assertion below exact.

use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::execution_engine::{ExecutionEngine, JitFunction};

use sdplc::codegen::{CodeGenerator, RuntimeVar, TIME_SETTER_FN};
use sdplc::lexer::Lexer;
use sdplc::parser::Parser;
use sdplc::semantic;

type VoidFn = unsafe extern "C" fn();
type GetterFn = unsafe extern "C" fn() -> f64;
type SetTimeFn = unsafe extern "C" fn(i64);

/// A compiled program under test, wired up for scan-by-scan execution.
struct Harness<'ctx> {
    engine: ExecutionEngine<'ctx>,
    vars: Vec<RuntimeVar>,
    program: String,
    now_ms: i64,
}

impl<'ctx> Harness<'ctx> {
    fn new(context: &'ctx Context, source: &str) -> Harness<'ctx> {
        let lexer = Lexer::new(source);
        let mut parser = Parser::new(lexer);
        let ast = parser.parse().expect("program should parse");

        let sem = semantic::analyze(ast.clone());
        assert!(
            !sem.has_errors(),
            "semantic errors: {:?}",
            sem.diagnostics
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
        );

        let mut codegen = CodeGenerator::new(context, "fb_test");
        let vars = codegen
            .compile_for_runtime(&ast)
            .expect("program should compile");
        let program = vars[0].program_name.clone();

        let engine = codegen
            .module()
            .create_jit_execution_engine(OptimizationLevel::None)
            .expect("JIT engine should start");

        let init: JitFunction<VoidFn> =
            unsafe { engine.get_function(&format!("__init_{}", program)) }
                .expect("init function should exist");
        unsafe { init.call() };

        Harness {
            engine,
            vars,
            program,
            now_ms: 0,
        }
    }

    /// Runs one scan cycle, having first advanced the scan clock by
    /// `elapsed_ms`.
    fn scan(&mut self, elapsed_ms: i64) {
        self.now_ms += elapsed_ms;
        if let Ok(set_time) =
            unsafe { self.engine.get_function::<SetTimeFn>(TIME_SETTER_FN) }
        {
            unsafe { set_time.call(self.now_ms) };
        }
        let scan: JitFunction<VoidFn> =
            unsafe { self.engine.get_function(&format!("__scan_{}", self.program)) }
                .expect("scan function should exist");
        unsafe { scan.call() };
    }

    /// Runs `count` scans of `elapsed_ms` each.
    fn scan_n(&mut self, count: usize, elapsed_ms: i64) {
        for _ in 0..count {
            self.scan(elapsed_ms);
        }
    }

    /// Writes a program variable through its generated setter.
    fn set(&self, name: &str, value: f64) {
        let var = self
            .vars
            .iter()
            .find(|v| v.name == name)
            .unwrap_or_else(|| panic!("no runtime variable '{}'", name));
        let setter_name = var
            .setter_fn_name
            .as_ref()
            .unwrap_or_else(|| panic!("'{}' is read-only", name));
        let setter: JitFunction<unsafe extern "C" fn(f64)> =
            unsafe { self.engine.get_function(setter_name) }.expect("setter should exist");
        unsafe { setter.call(value) };
    }

    fn set_bool(&self, name: &str, value: bool) {
        self.set(name, if value { 1.0 } else { 0.0 });
    }

    /// Reads a program variable, or a function block output such as
    /// `"t.ET"`, through its generated getter.
    fn get(&self, name: &str) -> f64 {
        let var = self
            .vars
            .iter()
            .find(|v| v.name == name)
            .unwrap_or_else(|| panic!("no runtime variable '{}'", name));
        let getter: JitFunction<GetterFn> =
            unsafe { self.engine.get_function(&var.getter_fn_name) }.expect("getter should exist");
        unsafe { getter.call() }
    }

    fn get_bool(&self, name: &str) -> bool {
        self.get(name) != 0.0
    }
}

// ─── TON ────────────────────────────────────────────────────────

#[test]
fn ton_holds_output_until_preset_elapses() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    start : BOOL := FALSE;
    t : TON;
    done : BOOL := FALSE;
END_VAR
t(IN := start, PT := T#500ms);
done := t.Q;
END_PROGRAM
"#,
    );

    // Input low: the timer is idle and its output stays off.
    h.scan_n(5, 100);
    assert!(!h.get_bool("done"), "TON fired with IN low");
    assert_eq!(h.get("t.ET"), 0.0, "TON accumulated time with IN low");

    // Input high: 400ms is not yet the 500ms preset.
    h.set_bool("start", true);
    h.scan(0);
    h.scan_n(4, 100);
    assert!(!h.get_bool("done"), "TON fired early at 400ms");
    assert_eq!(h.get("t.ET"), 400.0, "TON elapsed time wrong before preset");

    // Crossing the preset raises Q, and ET saturates at PT.
    h.scan(100);
    assert!(h.get_bool("done"), "TON did not fire at the preset");
    assert_eq!(h.get("t.ET"), 500.0, "TON ET should saturate at PT");

    h.scan_n(10, 100);
    assert!(h.get_bool("done"), "TON did not stay latched");
    assert_eq!(h.get("t.ET"), 500.0, "TON ET should stay clamped at PT");

    // Dropping the input resets both output and elapsed time at once.
    h.set_bool("start", false);
    h.scan(100);
    assert!(!h.get_bool("done"), "TON did not reset when IN dropped");
    assert_eq!(h.get("t.ET"), 0.0, "TON ET did not reset when IN dropped");
}

#[test]
fn ton_instances_keep_separate_state() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    a_in : BOOL := FALSE;
    b_in : BOOL := FALSE;
    fast : TON;
    slow : TON;
END_VAR
fast(IN := a_in, PT := T#100ms);
slow(IN := b_in, PT := T#1s);
END_PROGRAM
"#,
    );

    h.set_bool("a_in", true);
    h.set_bool("b_in", true);
    h.scan(0);
    h.scan_n(2, 100);

    assert!(h.get_bool("fast.Q"), "short timer should have expired");
    assert!(!h.get_bool("slow.Q"), "long timer should still be running");
    assert_eq!(h.get("fast.ET"), 100.0);
    assert_eq!(h.get("slow.ET"), 200.0);
}

// ─── TOF ────────────────────────────────────────────────────────

#[test]
fn tof_holds_output_after_input_falls() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    run : BOOL := FALSE;
    t : TOF;
END_VAR
t(IN := run, PT := T#300ms);
END_PROGRAM
"#,
    );

    h.set_bool("run", true);
    h.scan(0);
    assert!(h.get_bool("t.Q"), "TOF should follow IN up immediately");

    // Falling edge starts the hold-over period.
    h.set_bool("run", false);
    h.scan(100);
    assert!(h.get_bool("t.Q"), "TOF dropped immediately after IN fell");
    h.scan(100);
    assert!(h.get_bool("t.Q"), "TOF dropped before PT elapsed");
    assert_eq!(h.get("t.ET"), 100.0);

    h.scan(200);
    assert!(!h.get_bool("t.Q"), "TOF did not drop after PT elapsed");
    assert_eq!(h.get("t.ET"), 300.0, "TOF ET should saturate at PT");
}

// ─── TP ─────────────────────────────────────────────────────────

#[test]
fn tp_emits_one_fixed_length_pulse() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    trigger : BOOL := FALSE;
    t : TP;
END_VAR
t(IN := trigger, PT := T#200ms);
END_PROGRAM
"#,
    );

    h.scan(10);
    assert!(!h.get_bool("t.Q"), "TP fired without a trigger");

    // Rising edge starts the pulse.
    h.set_bool("trigger", true);
    h.scan(0);
    assert!(h.get_bool("t.Q"), "TP did not start on the rising edge");

    // The pulse is not retriggerable and does not extend with IN.
    h.scan(100);
    assert!(h.get_bool("t.Q"), "TP pulse ended early");
    h.scan(100);
    assert!(!h.get_bool("t.Q"), "TP pulse outlasted PT");
    assert_eq!(h.get("t.ET"), 200.0);

    // Holding IN high does not produce a second pulse.
    h.scan_n(5, 100);
    assert!(!h.get_bool("t.Q"), "TP retriggered while IN stayed high");
}

// ─── Counters ───────────────────────────────────────────────────

#[test]
fn ctu_counts_rising_edges_and_resets() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    pulse : BOOL := FALSE;
    clear : BOOL := FALSE;
    c : CTU;
END_VAR
c(CU := pulse, R := clear, PV := 3);
END_PROGRAM
"#,
    );

    // A level held high across scans is one edge, not many.
    h.set_bool("pulse", true);
    h.scan_n(5, 10);
    assert_eq!(h.get("c.CV"), 1.0, "CTU counted a level, not an edge");
    assert!(!h.get_bool("c.Q"));

    for _ in 0..2 {
        h.set_bool("pulse", false);
        h.scan(10);
        h.set_bool("pulse", true);
        h.scan(10);
    }
    assert_eq!(h.get("c.CV"), 3.0, "CTU miscounted edges");
    assert!(h.get_bool("c.Q"), "CTU did not assert Q at the preset");

    h.set_bool("clear", true);
    h.scan(10);
    assert_eq!(h.get("c.CV"), 0.0, "CTU did not clear on R");
    assert!(!h.get_bool("c.Q"));
}

#[test]
fn ctd_loads_and_counts_down_to_zero() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    pulse : BOOL := FALSE;
    load : BOOL := FALSE;
    c : CTD;
END_VAR
c(CD := pulse, LD := load, PV := 2);
END_PROGRAM
"#,
    );

    h.set_bool("load", true);
    h.scan(10);
    assert_eq!(h.get("c.CV"), 2.0, "CTD did not load PV");
    assert!(!h.get_bool("c.Q"));

    h.set_bool("load", false);
    for _ in 0..2 {
        h.set_bool("pulse", true);
        h.scan(10);
        h.set_bool("pulse", false);
        h.scan(10);
    }
    assert_eq!(h.get("c.CV"), 0.0, "CTD miscounted down");
    assert!(h.get_bool("c.Q"), "CTD did not assert Q at zero");
}

#[test]
fn ctud_counts_both_ways() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    up : BOOL := FALSE;
    down : BOOL := FALSE;
    c : CTUD;
END_VAR
c(CU := up, CD := down, R := FALSE, LD := FALSE, PV := 2);
END_PROGRAM
"#,
    );

    for _ in 0..3 {
        h.set_bool("up", true);
        h.scan(10);
        h.set_bool("up", false);
        h.scan(10);
    }
    assert_eq!(h.get("c.CV"), 3.0);
    assert!(h.get_bool("c.QU"), "CTUD did not assert QU past PV");
    assert!(!h.get_bool("c.QD"));

    for _ in 0..3 {
        h.set_bool("down", true);
        h.scan(10);
        h.set_bool("down", false);
        h.scan(10);
    }
    assert_eq!(h.get("c.CV"), 0.0);
    assert!(h.get_bool("c.QD"), "CTUD did not assert QD at zero");
}

// ─── Edge detectors ─────────────────────────────────────────────

#[test]
fn r_trig_fires_for_exactly_one_scan() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    signal : BOOL := FALSE;
    edge : R_TRIG;
    seen : DINT := 0;
END_VAR
edge(CLK := signal);
IF edge.Q THEN
    seen := seen + 1;
END_IF;
END_PROGRAM
"#,
    );

    h.scan_n(3, 10);
    assert_eq!(h.get("seen"), 0.0, "R_TRIG fired with no edge");

    h.set_bool("signal", true);
    h.scan_n(5, 10);
    assert_eq!(h.get("seen"), 1.0, "R_TRIG did not fire exactly once");

    h.set_bool("signal", false);
    h.scan_n(3, 10);
    h.set_bool("signal", true);
    h.scan_n(3, 10);
    assert_eq!(h.get("seen"), 2.0, "R_TRIG missed the second edge");
}

#[test]
fn f_trig_is_quiet_on_the_first_scan() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    signal : BOOL := FALSE;
    edge : F_TRIG;
    seen : DINT := 0;
END_VAR
edge(CLK := signal);
IF edge.Q THEN
    seen := seen + 1;
END_IF;
END_PROGRAM
"#,
    );

    // CLK starts FALSE — a naive implementation reports a phantom
    // falling edge here.
    h.scan_n(3, 10);
    assert_eq!(h.get("seen"), 0.0, "F_TRIG fired a phantom edge at startup");

    h.set_bool("signal", true);
    h.scan_n(2, 10);
    assert_eq!(h.get("seen"), 0.0, "F_TRIG fired on a rising edge");

    h.set_bool("signal", false);
    h.scan_n(4, 10);
    assert_eq!(h.get("seen"), 1.0, "F_TRIG did not fire exactly once");
}

// ─── Latches ────────────────────────────────────────────────────

#[test]
fn rs_latch_is_reset_dominant() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    set_in : BOOL := FALSE;
    reset_in : BOOL := FALSE;
    latch : RS;
END_VAR
latch(S := set_in, R1 := reset_in);
END_PROGRAM
"#,
    );

    h.set_bool("set_in", true);
    h.scan(10);
    assert!(h.get_bool("latch.Q1"), "RS did not set");

    h.set_bool("set_in", false);
    h.scan(10);
    assert!(h.get_bool("latch.Q1"), "RS did not hold after S dropped");

    // Both inputs high at once: reset must win.
    h.set_bool("set_in", true);
    h.set_bool("reset_in", true);
    h.scan(10);
    assert!(!h.get_bool("latch.Q1"), "RS is not reset-dominant");
}

#[test]
fn sr_latch_is_set_dominant() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    set_in : BOOL := FALSE;
    reset_in : BOOL := FALSE;
    latch : SR;
END_VAR
latch(S1 := set_in, R := reset_in);
END_PROGRAM
"#,
    );

    h.set_bool("reset_in", true);
    h.scan(10);
    assert!(!h.get_bool("latch.Q1"));

    // Both inputs high at once: set must win.
    h.set_bool("set_in", true);
    h.scan(10);
    assert!(h.get_bool("latch.Q1"), "SR is not set-dominant");

    h.set_bool("set_in", false);
    h.scan(10);
    assert!(!h.get_bool("latch.Q1"), "SR did not clear once S1 dropped");
}

// ─── Call conventions ───────────────────────────────────────────

#[test]
fn outputs_bind_through_arrow_arguments() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    start : BOOL := FALSE;
    t : TON;
    done : BOOL := FALSE;
    elapsed : TIME;
END_VAR
t(IN := start, PT := T#100ms, Q => done, ET => elapsed);
END_PROGRAM
"#,
    );

    h.set_bool("start", true);
    h.scan(0);
    h.scan(50);
    assert!(!h.get_bool("done"));
    assert_eq!(h.get("elapsed"), 50.0, "ET did not bind through '=>'");

    h.scan(50);
    assert!(h.get_bool("done"), "Q did not bind through '=>'");
    assert_eq!(h.get("elapsed"), 100.0);
}

#[test]
fn inputs_bind_positionally() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
PROGRAM P
VAR
    start : BOOL := FALSE;
    t : TON;
END_VAR
t(start, T#100ms);
END_PROGRAM
"#,
    );

    h.set_bool("start", true);
    h.scan(0);
    h.scan(60);
    assert!(!h.get_bool("t.Q"));
    h.scan(60);
    assert!(h.get_bool("t.Q"), "positional arguments did not bind");
}

#[test]
fn user_defined_function_blocks_keep_state() {
    let context = Context::create();
    let mut h = Harness::new(
        &context,
        r#"
FUNCTION_BLOCK Accumulator
VAR_INPUT
    step : REAL;
END_VAR
VAR_OUTPUT
    total : REAL;
END_VAR
    total := total + step;
END_FUNCTION_BLOCK

PROGRAM P
VAR
    acc : Accumulator;
    other : Accumulator;
END_VAR
acc(step := 1.5);
other(step := 10.0);
END_PROGRAM
"#,
    );

    h.scan_n(4, 10);
    assert_eq!(h.get("acc.total"), 6.0, "user FB did not accumulate");
    assert_eq!(
        h.get("other.total"),
        40.0,
        "user FB instances shared state"
    );
}
