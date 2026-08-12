# SD-PLC Design Guide — Path to Marketable MVP

This document is a build plan: what to add to SD-PLC so it can be sold as a bespoke
PLC solution, what to explicitly skip, and why — grounded in the thesis's own honesty
about its limitations (§6.4, §7.3).

The thesis's future-work list is ordered by *academic* value (what extends the research
contribution). This list is ordered by *commercial* value (what a paying industrial
client in a cost-sensitive market — Kenya, primarily — actually needs before they'll
trust SD-PLC on their plant floor).

The work is sequenced as six sprints. Each states what "done" means concretely, so
progress is checkable rather than a matter of opinion.

## Sprint Status

| # | Sprint | Status |
|---|---|---|
| 1 | Function Block Library | **Delivered** |
| 2 | Ladder Diagram (LD) Frontend | Not started |
| 3 | OPC UA Security (TLS + auth) | Not started |
| 4 | Deployment & Monitoring Workflow | Not started |
| 5 | Hardware-in-the-Loop Demo (Modicon M241) | Not started |
| 6 | Cross-Architecture Deployment Packaging | Not started |

Sprints 1 and 2 are strictly ordered — LD rungs are built from the same timers,
counters and latches Sprint 1 delivers. Sprints 3, 4 and 6 are independent of the
language work and can be reordered against client pressure. Sprint 5 depends on 2.

---

## Sprint 1 — Function Block Library Expansion — **DELIVERED**

**Why first:** No real ST program beyond a toy example avoids counters, latches and
edge detection. This was the gap between "compiles a thesis case study" and "compiles
what a client actually writes." It was also a prerequisite for Sprint 2 — Ladder
Diagram rungs lean on exactly these elements (timer coils, counter coils,
latch/unlatch coils), so the work is shared across both the ST and future LD
frontends.

**Correction to the original plan.** This document previously stated that "only `TON`
currently has runtime cooperation" and advised replicating its special-cased pattern
in `codegen.rs`. That was wrong: there was no `TON`, and no function block machinery
of any kind. `FUNCTION_BLOCK` compiled to `void @Name()` with stack-allocated
variables — correct for a stateless function, useless for a block whose entire purpose
is remembering where it was between scans. Instances declared as `t : TON;` resolved to
a placeholder `i64` and member access `t.Q` emitted a constant zero.

That turned out to be good news. Rather than ten special cases in the code generator,
the sprint built the general mechanism the language already implied, and then wrote the
standard blocks in Structured Text on top of it.

### What shipped

**Generic function block instances** (`src/codegen.rs`). Every `FUNCTION_BLOCK` now
compiles to an LLVM struct holding all its declarations, plus `void @Name(ptr %self)`
and `void @__fbinit_Name(ptr %self)`. Instances are structs — `alloca` in the AOT path,
module globals in the runtime path — so state survives scan cycles, and ten instances
mean ten independent state blocks. This applies to a user's own blocks exactly as it
does to the library's.

**The ten standard blocks** (`src/stdlib/standard_fb.st`), written in ST and compiled
through SD-PLC's own pipeline:

| Delivered | Blocks |
|---|---|
| Timers | `TON`, `TOF`, `TP` |
| Counters | `CTU`, `CTD`, `CTUD` |
| Edge detectors | `R_TRIG`, `F_TRIG` |
| Latches | `RS` (reset-dominant), `SR` (set-dominant) |

Nothing in `codegen.rs` names any of them. Adding an eleventh block is an edit to one
`.st` file and a test — no Rust.

**Call and member syntax** across the front end: `t(IN := x, PT := T#5s)` with
positional, named and `=>` output binding; `t.Q` and `t.ET` as readable members, and as
assignment targets. Semantic analysis validates parameter names, directions and types
rather than warning that the callee is unknown.

**`TIME` as a real type.** `TIME` is a signed 64-bit millisecond count.
`T#1h30m`, `T#2m10s500ms`, `T#1_500ms` and `T#-2s` all parse to constants; duration
arithmetic and comparison type-check. Previously every temporal literal emitted `i64 0`.

**A deterministic scan clock.** Timers read `TIME_MS()`, which lowers to a load of the
`@__sdplc_now_ms` global, emitted lazily along with a `__sdplc_set_time_ms(i64)` setter
— a program with no timers carries neither symbol. The runtime samples the clock once
per cycle and publishes it *before* the scan body, so all timers in a scan observe the
same instant. Timers advance in whole scan cycles, which is what keeps runs
reproducible.

**Function block outputs on the dashboard.** Instance outputs are exposed as runtime
variables named `instance.field` (`warmup.Q`, `parts.CV`), so they appear in the
terminal dashboard, the CSV artefacts and the OPC UA address space with no extra work.
They are read-only: the scan cycle owns them.

### How it was verified

`tests/function_block_test.rs` — 14 tests that JIT-execute compiled programs and drive
the scan clock by hand, asserting plant-visible behaviour rather than IR text: a `TON`
still off at 400ms of a 500ms preset and on at 500ms with `ET` saturating; `TOF`
holding over a falling edge; a `TP` pulse that cannot be retriggered; a `CTU` counting
an edge rather than a held level; `F_TRIG` staying quiet on scan 1; `RS` losing to
reset and `SR` winning on set; two instances of a user-defined block not sharing state.

Eight new semantic tests cover the diagnostics. `programs/fb_library_demo.st` exercises
every block in one conveyor sequence and runs on the real runtime. Suite total: **171
tests, all passing**; the demo's IR compiles under `llc` for x86-64, AArch64 and
ARMv5TE.

Full architecture write-up: `Developer_Guide.md` §13.

### What was deliberately left out

- **`RETAIN` semantics for instances.** Instance state is zeroed at `__init_`; there is
  no persistence across a restart. Nothing in the runtime persists anything yet, so
  this belongs with Sprint 4's deployment work, not here.
- **Nested instance initial values.** A block holding another block's instance lays out
  and zeroes correctly, but an initial value on a nested field is not propagated.
  No standard block needs it.
- **`DATE` / `TOD` / `DT` literals** still emit `i64 0`. `TIME` was what the timers
  needed; calendar arithmetic has no buyer-facing use case yet.

---

## Sprint 2 — Ladder Diagram (LD) Frontend

**Why now:** Industry — especially the kind of "simple machine" automation common on
cost-sensitive plant floors — overwhelmingly favours Ladder Diagram over Structured
Text. An ST-only tool is a hard sell to a technician trained on rungs and contacts,
regardless of how good the compiler backend is. This was originally scoped as
low-priority future work; it is now a commercial priority, not just a research
extension.

**Why this is architecturally tractable, not a rewrite:** SD-PLC's AST is already the
shared contract between frontends and the compiler backend — the thesis's own
multi-language design note states that all three graphical languages would produce the
same AST the ST frontend does. Concretely:

```
LD source (PLCopen XML rungs)  ──┐
ST source (.st text)           ──┼──▶  CompilationUnit (AST)  ──▶  semantic.rs ──▶ codegen.rs
FBD / SFC (future)             ──┘        (unchanged)              (unchanged)      (unchanged)
```

Nothing downstream of the AST needs to change. This is a frontend-only build — and
Sprint 1 makes that literally true for coils, which now have real blocks to target.

**What to add, in order:**

1. **PLCopen XML ingestion for LD networks** (`IEC 61131-10` interchange format).
   A design document for this already exists per the thesis's future-work notes —
   implement against it rather than starting from scratch.
2. **Rung → AST translation.** A rung is a left-to-right, top-to-bottom evaluation of
   contacts (series = AND, parallel branches = OR) driving coils (assignment) or FB
   instances. This maps onto existing `Expression::BinaryOp` and
   `Statement::Assignment`/`CallStatement` nodes — no new AST variants should be needed
   for basic rungs.
3. **Special coil types** — set/reset coils, one-shot coils. These now map directly
   onto the shipped `RS`/`SR` and `R_TRIG`/`F_TRIG` blocks: a set coil becomes an `SR`
   instance call, a one-shot becomes an `R_TRIG`. Sprint 1 is what makes this a
   translation rather than a second implementation.
4. **Author and validate LD test input using EcoStruxure Machine Expert on the Modicon
   M241** (see benchmarking below) — real, vendor-produced PLCopen XML, rather than
   hand-built or third-party editor XML of uncertain fidelity to what industrial tools
   emit.

**Where this lands in the codebase:**

- New module `src/plcopen.rs`, parsing XML and producing a `CompilationUnit` — the
  extension point already noted in `Developer_Guide.md` §12 ("I want to add PLCopen XML
  input").
- `main.rs` routes `.xml` input to this frontend instead of `Lexer` + `Parser`;
  everything from semantic analysis onward is untouched. Note that `stdlib::inject()`
  runs inside `semantic::analyze()` and the codegen entry points, so an LD-derived AST
  gets the standard blocks automatically — the new frontend does not have to know the
  library exists.
- Add an integration test category parallel to the existing suites, seeded from real
  PLCopen XML rung examples exported from Machine Expert.

**Done when:** a rung authored in Machine Expert, exported as PLCopen XML, compiles to
IR whose behaviour matches the same rung running under CODESYS on a captured trace.

**Scope discipline:** LD only, not FBD or SFC. FBD/SFC can reuse the same ingestion
pattern later if a client need appears.

---

## Sprint 3 — OPC UA Security (TLS + Basic Auth)

**Why:** An unsecured OPC UA endpoint is an immediate, obvious objection from any real
buyer — even a cost-sensitive one. This is comparatively cheap relative to the
credibility it buys.

**What to add:**

- TLS transport (certificate + private key configuration on the server)
- A minimal username/password or certificate-based authentication policy
- Certificate-store handling documented clearly enough that a non-Rust-developer
  technician could configure it per deployment

**Where this lands:** The OPC UA server is pure-Rust (per thesis §4.5) exposing
`Objects/SDPLC` with read/write callbacks over an unsecured transport, used that way
specifically for the latency measurements. Security-policy configuration is a
server-startup concern, not a runtime-loop concern — it should not touch `codegen.rs`
or the scan-cycle logic at all. Isolate it to the OPC UA server setup code.

**Done when:** the server rejects an anonymous unencrypted client, accepts a
credentialed encrypted one, and the latency figures are re-measured under TLS so the
security cost is quantified rather than assumed.

**Do not** attempt full O-PAS profile-level security certification here — that's the
explicitly out-of-scope item from §6.4. The bar is "a client's IT/OT team doesn't
reject it on sight," not formal conformance.

---

## Sprint 4 — Minimal Deployment & Monitoring Workflow

**Why:** Running SD-PLC currently means using the CLI and a terminal dashboard. That's
fine for you; it's not fine for a client's technician, and it's not fine for a sales
demo.

**What to add:**

- A way to push a compiled program to target hardware without a Rust toolchain on-site
  — a packaged binary plus a one-line deploy script per architecture, not a
  rebuild-from-source step.
- A lightweight local web dashboard reading live variables via the OPC UA server (not a
  new data path — an OPC UA client in a browser-friendly wrapper) to replace the
  terminal dashboard for anything client-facing.
- **`RETAIN` persistence**, deferred here from Sprint 1: retained variables and function
  block instance state written to storage on shutdown and restored on start. This is a
  deployment concern — it needs somewhere to persist *to* — and it is what a client
  means when they ask whether the machine remembers its position after a power cut.

**Where this lands:** The terminal dashboard in `runtime.rs` already enumerates
`RuntimeVar`s and formats them by type, including the `instance.field` entries Sprint 1
added for function block outputs. A web dashboard is mostly a presentation-layer
wrapper around data that already exists — it should consume the OPC UA server's exposed
namespace rather than duplicating runtime internals.

**Done when:** someone who is not you can deploy a compiled program to a Raspberry Pi
and watch it run in a browser, working only from written instructions.

**Explicitly not this:** a full engineering IDE (see the skip list). The bar is "a
technician can deploy and watch it," not "a technician can develop in it."

---

## Sprint 5 — Hardware-in-the-Loop Demo (Modicon M241 as field device)

**Why:** Simulation-based validation is the most named limitation (§6.4). One working
physical demonstration — even small — outweighs any amount of additional CSV evidence
when you're standing in front of a buyer.

**What to build:**

- Raspberry Pi 4 (already a validated target architecture) as the SD-PLC runtime host,
  communicating over Modbus to a **Schneider Modicon M241** acting as the real field I/O
  device. The M241 is not a compilation target — SD-PLC cannot run on its proprietary
  firmware — but it speaks Modbus RTU (serial) and Modbus TCP (Ethernet), and that
  Modbus link is the integration point.
- SD-PLC (running compiled ST, or LD-derived logic once Sprint 2 lands) becomes the
  Modbus master/client; the M241's real digital/analog I/O is the physical plant
  interface.
- Capture the same timing/jitter evidence already produced in simulation, but from real
  I/O, so the claim becomes "this held determinism on physical hardware," not "this held
  determinism in a deterministic simulation."

**Why the M241 specifically, over a cheaper nano PLC:** the M241 runs the full
EcoStruxure Machine Expert environment, which is CODESYS-based and supports all five
IEC 61131-3 languages with genuine PLCopen XML export/import — unlike the entry-level
M221, which offers only IL/LD/Grafcet through a separate, cut-down tool with no PLCopen
XML interchange. Since the existing CODESYS golden-reference validation pipeline
(thesis §5.x trace capture) already assumes a CODESYS-family engineering tool, the M241
extends that pipeline to real hardware rather than requiring a second, incompatible
validation path.

**Why this matters commercially:** Schneider PLCs are a common installed base in the
target markets. A demo of SD-PLC driving real Schneider I/O over Modbus, validated
against logic authored in the same CODESYS-derived environment used across the
industrial giants, is a concrete "this integrates with and is interchangeable with what
you already have" story.

**Where this lands:** No new compiler or runtime code needed in principle — this is a
Modbus master integration exercise, not a language feature. Keep the control logic
simple; the point is proving the runtime/hardware/protocol path.

**Done when:** a scan-timing CSV exists that was produced with real I/O in the loop, and
its jitter distribution stands up next to the simulation figures.

---

## Sprint 6 — Cross-Architecture Deployment Packaging

**Why:** Cross-architecture portability is SD-PLC's actual differentiator versus
vendor-locked competitors. Right now that claim is proven by you personally compiling
for four targets — it needs to be proven by a repeatable process someone else could
follow.

**What to add:**

- A documented, scripted build path per target (x86_64, Jetson Orin Nano ARMv8.2-A,
  Raspberry Pi 4 ARMv8-A, Nuvoton NUC980 ARMv5TE) using `llc` with the correct
  `-mtriple`. The compiler already emits only architecture-independent LLVM IR, so no
  compiler changes are needed — this is a build-script and documentation task.
- A short README aimed at "someone deploying this, not someone developing it."
- The script must cover the runtime support symbols, not just the program: a host
  embedding compiled `.ll`/`.bc` has to call `__sdplc_set_time_ms()` once per scan or
  every timer in the program stays frozen at zero. That contract needs to be in the
  deployment README, not only in `Developer_Guide.md` §13.4.

**Where this lands:** This formalises the "I want to target a new architecture" note in
`Developer_Guide.md` §12 — turning a developer note into a client-facing packaging
script.

**Done when:** a clean machine with only the scripted prerequisites produces working
binaries for all four targets.

---

## External Benchmarks, Comparison & Inspiration

These are not build targets — they're reference material to test against and learn
from, the same way CODESYS is the golden reference for correctness. Two tiers:

**Software reference — OSCAT / STMutants (ST and LD logic patterns)**

- OSCAT is a well-established, widely used open-source IEC 61131-3 controls library —
  real-world ST idioms, not something written in-house.
- A curated mutation-testing dataset (STMutants) draws 11 ST programs from the OSCAT
  basic library and industrial sources, 38–211 lines each, deliberately covering complex
  control flow, internal state retention, timer-dependent behaviour and numeric
  precision — a ready-made corpus rather than something you'd have to hand-pick and
  defend the choice of.
- Use as: compilation test inputs (does SD-PLC accept real-world idioms, not just the
  case study?) and as evidence for which blocks beyond Sprint 1's ten are actually
  common in practice. Sprint 1 makes this newly viable — a corpus emphasising "internal
  state retention" and "timer-dependent behaviour" would have failed to compile at all
  before it.
- Licensing note: OSCAT is GPL-family licensed — use it as test input and cite it; don't
  redistribute or reproduce its source in marketing material or thesis-adjacent
  documents.

**Hardware/vendor reference — Modicon M241 (LD, PLCopen XML, physical validation)**

- The M241 runs EcoStruxure Machine Expert (CODESYS-based), supporting the full
  IEC 61131-3 language set and genuine PLCopen XML export/import — a legitimate
  industry-giant-tier reference, not an entry-level approximation of one.
- Author rung logic in Machine Expert, export as PLCopen XML, and use that as ingestion
  test input for the Sprint 2 LD frontend — a much stronger claim than testing against
  hand-built XML, since it proves compatibility with what an actual Schneider
  engineering tool emits.
- Once Sprints 2 and 5 both exist, the M241 can serve a second role: run the same rung
  logic physically on the M241 and compare its I/O behaviour against SD-PLC's own
  LD-derived logic — polled over Modbus, or via the existing CODESYS-family
  trace-capture discipline if the environment supports it on this hardware.
- Keep this distinction explicit in documentation: CODESYS (in simulation) remains the
  correctness golden reference from the thesis; the M241 extends that same CODESYS-family
  lineage onto physical hardware and into the LD/PLCopen interchange path.

Run the OSCAT/STMutants and M241 benchmarking work alongside Sprints 1–2 rather than as
a separate phase — it's most useful as a source of realistic test cases while those
frontends are actively being built, not as a final check afterward.

---

## Explicitly Unnecessary for MVP Marketability

Keep these out of scope for now — each is either high-effort/low-buyer-relevance at this
stage, or requires resources (accredited certification bodies, large parallel engineering
effort) that don't make sense pre-revenue:

| Item | Why it's skipped for now |
|---|---|
| FBD / SFC support | LD is the industry-favoured graphical language for this market; FBD/SFC can reuse the same PLCopen XML ingestion pattern later if a client need appears |
| Formal O-PAS / IEC 62443 certification | Requires accredited third-party assessment (TÜV, exida, The Open Group) — not viable pre-revenue |
| Docker / Kubernetes orchestration | Irrelevant to standalone industrial deployments |
| Full ladder-diagram authoring/editor UI | Not needed at MVP stage — ingest PLCopen XML produced by an existing engineering tool (Machine Expert on the M241) rather than building an editor |
| WebAssembly target | No current buyer need |
| Full IEC 61131-3 language coverage | Diminishing returns versus targeted function block (Sprint 1) and LD (Sprint 2) work |
| Remaining standard library beyond the ten blocks | The ten shipped in Sprint 1 cover the vast majority of real ST and every LD rung element. Extend on evidence from the OSCAT corpus, not speculatively — the cost is now one `.st` edit per block |
| `DATE`/`TOD`/`DT` arithmetic | `TIME` was what timers needed; calendar types have no buyer-facing use case yet |
| Modicon M221 as primary benchmark hardware | Entry-level tier offers only IL/LD/Grafcet via a cut-down tool with no PLCopen XML interchange — insufficient for benchmarking against industry-giant-tier tooling; the M241 replaces it in that role |
