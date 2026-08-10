# SD-PLC Design Guide — Path to Marketable MVP

This document is a build plan:
what to add to SD-PLC so it can be sold as a bespoke PLC solution, what to explicitly
skip, and why — grounded in the thesis's own honesty about its limitations (§6.4, §7.3).

The thesis's future-work list is ordered by *academic* value (what extends the research
contribution). This list is ordered by *commercial* value (what a paying industrial
client in a cost-sensitive market — Kenya, primarily — actually needs before they'll
trust SD-PLC on their plant floor).

---

## Priority 1 — Function Block Library Expansion

**Why first:** Only `TON` currently has runtime cooperation. No real ST program beyond
a toy example avoids counters, latches, and edge detection. This is the gap between
"compiles a thesis case study" and "compiles what a client actually writes." It is
also a prerequisite for Priority 2 — Ladder Diagram rungs lean on exactly these same
elements (timer coils, counter coils, latch/unlatch coils), so this work is shared
across both the ST and future LD frontends.

**What to add**, in order of how often they appear in real ST code:
- `TOF` (off-delay timer) and `TP` (pulse timer) — same shape as `TON`, reuse its
  runtime-cooperation pattern.
- `CTU`, `CTD`, `CTUD` (up/down counters)
- `R_TRIG`, `F_TRIG` (edge detectors)
- `RS`, `SR` (set/reset latches — note IEC 61131-3 defines both dominant-set and
  dominant-reset variants; pick one and document it clearly)

**Where this lands in the codebase:**
- `TON` is currently handled as a special-cased function block with runtime
  cooperation — find that pattern (search `codegen.rs` for the TON handling) and
  replicate its shape for each new FB rather than inventing a new mechanism.
- Each FB needs: an internal state struct (persisted like `RuntimeVar` module
  globals), instantiation semantics (multiple instances = multiple state blocks),
  and semantic-analysis awareness so `semantic.rs` accepts calls to it with the
  right parameter types.
- Add one test per FB in the existing `codegen_integration_test.rs` pattern —
  check the generated IR contains the expected state transitions, not just that
  it compiles.

**Effort signal:** This is the most repetitive of the priority items — once TOF/TP
are done (nearly identical to TON), counters and edge detectors are smaller each.
Budget this as the biggest single chunk of pre-sale engineering time.

---

## Priority 2 — Ladder Diagram (LD) Frontend

**Why elevated:** Industry — especially the kind of "simple machine" automation
common on cost-sensitive plant floors — overwhelmingly favours Ladder Diagram over
Structured Text. An ST-only tool is a hard sell to a technician trained on rungs and
contacts, regardless of how good the compiler backend is. This was originally scoped
as low-priority future work; it is now a commercial priority, not just a research
extension.

**Why this is architecturally tractable, not a rewrite:** SD-PLC's AST is already
designed to be the shared contract between frontends and the compiler backend — the
thesis's own multi-language design note states that all three graphical languages
would produce the same AST that the ST frontend does. Concretely:

```
LD source (PLCopen XML rungs)  ──┐
ST source (.st text)           ──┼──▶  CompilationUnit (AST)  ──▶  semantic.rs ──▶ codegen.rs
FBD / SFC (future)             ──┘        (unchanged)              (unchanged)      (unchanged)
```

Nothing downstream of the AST needs to change. This is a frontend-only build.

**What to add, in order:**
1. **PLCopen XML ingestion for LD networks** (`IEC 61131-10` interchange format).
   Design document for this already exists per the thesis's future-work notes —
   implement against it rather than starting from scratch.
2. **Rung → AST translation.** A rung is a left-to-right, top-to-bottom evaluation
   of contacts (series = AND, parallel branches = OR) driving coils (assignment) or
   FB instances (timers/counters from Priority 1). This maps cleanly onto existing
   `Expression::BinaryOp` and `Statement::Assignment`/`CallStatement` nodes — no new
   AST variants should be needed for basic rungs.
2b. **Special coil types**: set/reset coils, one-shot coils — these map to the RS/SR
   and R_TRIG/F_TRIG function blocks from Priority 1, reinforcing why that work
   comes first.
3. **Author and validate LD test input using EcoStruxure Machine Expert on the
   Modicon M241** (see benchmarking section below) — this gives you real,
   vendor-produced PLCopen XML to ingest, rather than hand-built or third-party
   editor XML of uncertain fidelity to what industrial tools actually emit.

**Where this lands in the codebase:**
- New module, e.g. `src/plcopen.rs`, parsing XML and producing a `CompilationUnit` —
  this is exactly the extension point already noted in `Developer_Guide.md` §12
  ("I want to add PLCopen XML input").
- `main.rs` routes `.xml` input to this new frontend instead of `Lexer` + `Parser`;
  everything from semantic analysis onward is untouched.
- Add an integration test category parallel to the existing lexer/parser/semantic/
  codegen suites, but seeded from real PLCopen XML rung examples exported from
  Machine Expert rather than `.st` source.

**Scope discipline:** Target LD only for this pass, not FBD or SFC — LD is the
industry-favoured language and the one worth the investment now. FBD/SFC can reuse
the same PLCopen XML ingestion pattern later if a client need appears.

---

## Priority 3 — OPC UA Security (TLS + Basic Auth)

**Why third:** An unsecured OPC UA endpoint is an immediate, obvious objection from
any real buyer — even a cost-sensitive one. This is comparatively cheap relative to
the credibility it buys.

**What to add:**
- TLS transport (certificate + private key configuration on the server)
- A minimal username/password or certificate-based authentication policy
- Certificate-store handling documented clearly enough that a non-Rust-developer
  technician could configure it per deployment

**Where this lands:** Your OPC UA server is pure-Rust (per thesis §4.5) exposing
`Objects/SDPLC` with read/write callbacks over an unsecured transport, used that way
specifically for the latency measurements. The security-policy configuration is a
server-startup concern, not a runtime-loop concern — it shouldn't touch `codegen.rs`
or the scan-cycle logic at all. Isolate it to the OPC UA server setup code.

**Do not** attempt full O-PAS profile—level security certification here — that's the
explicitly-out-of-scope item from §6.4. This is "good enough that a client's IT/OT
team doesn't reject it on sight," not formal conformance.

---

## Priority 4 — Minimal Deployment & Monitoring Workflow

**Why fourth:** Right now, running SD-PLC means using the CLI and a terminal
dashboard. That's fine for you; it's not fine for a client's technician, and it's not
fine for a sales demo.

**What to add:**
- A simple way to push a compiled program to target hardware without a Rust
  toolchain on-site — e.g. a packaged binary + a one-line deploy script per
  architecture, not a rebuild-from-source step.
- A lightweight local web dashboard that reads live variables via the OPC UA server
  (not a new data path — just an OPC UA client in a browser-friendly wrapper) to
  replace the terminal dashboard for anything client-facing.

**Where this lands:** The terminal dashboard in `runtime.rs` already knows how to
enumerate `RuntimeVar`s and format them by type (BOOL/INT/REAL). A web dashboard is
mostly a presentation-layer wrapper around data that already exists — it should
consume the OPC UA server's exposed namespace rather than duplicating the runtime's
internals.

**Explicitly not this:** a full engineering IDE (see "Skip" list below). The bar
here is "a technician can deploy and watch it," not "a technician can develop in it."

---

## Priority 5 — Hardware-in-the-Loop Demo (with Modicon M241 as field device and LD/PLCopen reference)

**Why fifth:** Simulation-based validation is your most named limitation (§6.4).
One working physical demonstration — even small — outweighs any amount of additional
CSV evidence when you're standing in front of a buyer.

**What to build:**
- Raspberry Pi 4 (already a validated target architecture) as the SD-PLC runtime
  host, communicating over Modbus to a **Schneider Modicon M241** acting as the real
  field I/O device. The M241 is not a compilation target — SD-PLC cannot run on its
  proprietary firmware — but it communicates over Modbus RTU (serial) and Modbus TCP
  (Ethernet), and that Modbus link is the integration point.
- SD-PLC (running your compiled ST or LD-derived logic, once Priority 2 lands)
  becomes the Modbus master/client; the M241's real digital/analog I/O is the
  physical plant interface.
- Capture the same kind of timing/jitter evidence you already produce in simulation,
  but from real I/O, so you can show "this held determinism on physical hardware,"
  not just "this held determinism in a deterministic simulation."

**Why the M241 specifically, over a cheaper nano PLC:** the M241 runs the full
EcoStruxure Machine Expert environment, which is CODESYS-based and supports all five
IEC 61131-3 languages (LD, FBD, ST, SFC, IL) with genuine PLCopen XML export/import —
unlike the entry-level M221, which only offers IL/LD/Grafcet through a separate,
cut-down tool with no PLCopen XML interchange. Since your existing CODESYS
golden-reference validation pipeline (thesis §5.x trace capture) already assumes a
CODESYS-family engineering tool, the M241 extends that pipeline to real hardware
rather than requiring a second, incompatible validation path.

**Why this demo matters commercially:** Schneider PLCs are a common installed base
in the industrial markets you're targeting. A demo of SD-PLC driving real Schneider
I/O over Modbus, validated against logic authored in the same CODESYS-derived
environment used across the industrial giants, is a concrete, credible "this
integrates with and is interchangeable with what you already have" story.

**Where this lands:** No new compiler or runtime code needed in principle — this is
a Modbus master integration exercise, not a language feature. Keep the control logic
itself simple; the point is proving the runtime/hardware/protocol path.

---

## Priority 6 — Cross-Architecture Deployment Packaging

**Why sixth:** Cross-architecture portability is SD-PLC's actual differentiator
versus vendor-locked competitors. Right now that claim is proven by you personally
compiling for four targets — it needs to be proven by a repeatable process someone
else could follow.

**What to add:**
- A documented, scripted build path per target (x86_64, Jetson Orin Nano ARMv8.2-A,
  Raspberry Pi 4 ARMv8-A, Nuvoton NUC980 ARMv5TE) using `llc` with the correct
  `-mtriple` per target (the compiler already only emits architecture-independent
  LLVM IR, so no compiler changes are needed — this is a build-script/documentation
  task).
- A short README aimed at "someone deploying this, not someone developing it."

**Where this lands:** This directly formalizes the "I want to target a new
architecture" note already in `Developer_Guide.md` §12 — turn that developer note
into a client-facing packaging script.

---

## External Benchmarks, Comparison & Inspiration

These are not build targets — they're reference material to test against and learn
from, the same way CODESYS is your golden reference for correctness. Two tiers:

**Software reference — OSCAT / STMutants (for ST and LD logic patterns)**
- OSCAT is a well-established, widely used open-source IEC 61131-3 controls library
  — real-world ST (and other language) idioms, not something you wrote yourself.
- A curated mutation-testing dataset (STMutants) draws 11 ST programs from the OSCAT
  basic library and industrial sources, 38–211 lines each, deliberately covering
  complex control flow, internal state retention, timer-dependent behaviour, and
  numeric precision — a ready-made corpus rather than something you'd have to
  hand-pick and defend the choice of.
- Use as: compilation test inputs (does SD-PLC accept real-world idioms, not just
  your own case study?) and as inspiration for which FBs/patterns in Priority 1 are
  actually common in practice.
- Licensing note: OSCAT is GPL-family licensed — use it as a test input and cite it,
  don't redistribute or reproduce its source in your own marketing material or
  thesis-adjacent documents.

**Hardware/vendor reference — Modicon M241 (LD, PLCopen XML, and physical
validation)**
- The M241 runs EcoStruxure Machine Expert (CODESYS-based), supporting the full
  IEC 61131-3 language set and genuine PLCopen XML export/import — making it a
  legitimate industry-giant-tier reference, not an entry-level approximation of one.
- Author rung logic in Machine Expert, export it as PLCopen XML, and use that as
  real-world ingestion test input for the Priority 2 LD frontend — this is a much
  stronger claim than testing against hand-built or third-party-editor XML, since
  it proves compatibility with what an actual Schneider engineering tool emits.
- Once Priority 2 (LD ingestion) and Priority 5 (HIL demo) both exist, the M241 can
  serve a second role: run the same rung logic physically on the M241, and compare
  its I/O behaviour against SD-PLC's own LD-derived logic — polled over Modbus, or
  via your existing CODESYS-family trace-capture discipline if the environment
  supports it on this hardware.
- Keep this distinction explicit in any documentation: CODESYS (in simulation)
  remains the correctness golden reference from the thesis; the M241 extends that
  same CODESYS-family lineage onto physical hardware and into the LD/PLCopen
  interchange path.

---

## Explicitly Unnecessary for MVP Marketability

Keep these out of scope for now — each is either high-effort/low-buyer-relevance at
this stage, or requires resources (accredited certification bodies, large parallel
engineering effort) that don't make sense pre-revenue:

| Item | Why it's skipped for now |
|---|---|
| FBD / SFC support | LD is the industry-favoured graphical language for this market; FBD/SFC can reuse the same PLCopen XML ingestion pattern later if a client need appears |
| Formal O-PAS / IEC 62443 certification | Requires accredited third-party assessment (TÜV, exida, The Open Group) — not viable pre-revenue |
| Docker / Kubernetes orchestration | Irrelevant to standalone industrial deployments |
| Full ladder-diagram authoring/editor UI | Not needed at MVP stage — ingest PLCopen XML produced by an existing engineering tool (Machine Expert on the M241) rather than building your own editor |
| WebAssembly target | No current buyer need |
| Full IEC 61131-3 language coverage | Diminishing returns versus targeted FB (Priority 1) and LD (Priority 2) work |
| Modicon M221 as primary benchmark hardware | Entry-level tier only offers IL/LD/Grafcet via a cut-down tool with no PLCopen XML interchange — insufficient for LD/PLCopen benchmarking against industry-giant-tier tooling; the M241 replaces it in that role |

---

## Suggested Sequencing

1. Function block library (Priority 1) — unblocks realistic ST programs *and* is a
   prerequisite for LD coils/rung elements
2. Ladder Diagram frontend (Priority 2) — matches industry preference; reuses the
   existing AST/backend unchanged
3. OPC UA security (Priority 3) — removes an immediate objection, low effort
4. Deployment/monitoring workflow (Priority 4) — makes it demoable by someone who
   isn't you
5. Hardware-in-the-loop demo with the M241 as a Modbus field device and PLCopen
   reference (Priority 5) — the single strongest credibility proof, and a real,
   industry-tier interoperability story
6. Deployment packaging (Priority 6) — operationalizes the portability claim

Run the OSCAT/STMutants and M241 benchmarking work alongside Priorities 1–2 rather
than as a separate phase — it's most useful as a source of realistic test cases
while those frontends are actively being built, not as a final check afterward.