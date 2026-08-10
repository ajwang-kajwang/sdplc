# SD-PLC Developer Guide
 
## How the Compiler Works, File by File
 
This document traces exactly what happens when you run `sdplc programs/my_program.st` — from the moment the `.st` file is read from disk to the moment `output.ll` is written. Every function, every data structure, every line number.
 
---
 
## 1. Project Map
 
```
sdplc/
├── Cargo.toml                  ← Rust package config, inkwell dependency
├── README.md                   ← User-facing documentation
│
├── docs/
│   ├── developer_guide.md        ← This file
│   └── multi_language_design.md  ← Future: LD/FBD/SFC via PLCopen XML
│
├── examples/                   ← Reference .st programs shipped with the repo
│   ├── hello.st                   Minimal program (start here)
│   ├── pid_controller.st         PID function block + temperature control
│   ├── multi_pou.st              Functions, FBs, and programs together
│   ├── all_control_flow.st       Every ST control flow construct
│   └── flotation_column.st       Thesis validation target (200+ lines)
│
├── programs/                   ← YOUR working .st files (not tracked)
│
├── src/
│   ├── main.rs      [259 lines]  CLI driver — reads .st, runs 4 stages
│   ├── bin/
│   │   └── runtime.rs [373 lines] JIT scan cycle executor + dashboard
│   ├── lib.rs       [ 51 lines]  Crate root — pub mod declarations
│   ├── lexer.rs     [1163 lines] Stage 1: source text → tokens
│   ├── parser.rs    [1347 lines] Stage 2: tokens → AST
│   ├── ast.rs       [ 383 lines] Shared data model (AST node types)
│   ├── semantic.rs  [1363 lines] Stage 3: AST → validated + typed AST
│   └── codegen.rs   [1627 lines] Stage 4: AST → LLVM IR (+ runtime compilation)
│
└── tests/
    ├── lexer_integration_test.rs      [10 tests]
    ├── parser_integration_test.rs     [ 8 tests]
    ├── semantic_integration_test.rs   [18 tests]
    └── codegen_integration_test.rs    [11 tests]
```
 
Total: **7,712 lines of Rust**, **137 tests**, **460 lines of example ST**, **2 binaries**.
 
---
 
## 2. Data Flow Overview
 
When you type `sdplc programs/my_program.st`, data transforms through four stages:
 
```
 ┌─────────────────────────────────────────────────────────────────┐
 │  programs/my_program.st  (file on disk)                        │
 └────────────────────┬────────────────────────────────────────────┘
                      │
         main.rs:68   │  fs::read_to_string(path)
                      │  Produces: String (raw source code)
                      ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │  Stage 1: LEXER  (lexer.rs)                                    │
 │  Input:  &str (source code)                                    │
 │  Output: Vec<Token>  (token stream)                            │
 │                                                                │
 │  "IF count >= limit THEN"                                      │
 │    → [If, Ident("count"), GreaterEq, Ident("limit"), Then]     │
 └────────────────────┬────────────────────────────────────────────┘
                      │
       main.rs:120    │  Parser::new(lexer) consumes the Lexer
                      │  parser.rs:54 calls lexer.tokenize()
                      ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │  Stage 2: PARSER  (parser.rs)                                  │
 │  Input:  Vec<Token>                                            │
 │  Output: CompilationUnit  (AST, defined in ast.rs)             │
 │                                                                │
 │  [If, Ident("count"), GreaterEq, Ident("limit"), Then, ...]    │
 │    → Statement::If {                                           │
 │        condition: BinaryOp { Ge, Identifier("count"),          │
 │                                   Identifier("limit") },       │
 │        then_body: [...],                                       │
 │      }                                                         │
 └────────────────────┬────────────────────────────────────────────┘
                      │
       main.rs:137    │  semantic::analyze(ast.clone())
                      ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │  Stage 3: SEMANTIC ANALYSIS  (semantic.rs)                     │
 │  Input:  CompilationUnit (AST)                                 │
 │  Output: ProgramContext { ast, symbols, diagnostics }          │
 │                                                                │
 │  Checks: "count" is declared? Type = INT ✓                    │
 │          "limit" is declared? Type = INT ✓                    │
 │          INT >= INT → produces BOOL ✓                         │
 │          IF condition is BOOL ✓                               │
 └────────────────────┬────────────────────────────────────────────┘
                      │
                      ├──── Compiler path (main.rs) ──────┐
       main.rs:158    │  codegen.compile(&ast)             │
                      ▼                                    │
         output.ll  +  output.bc                           │
                                                           │
                      ├──── Runtime path (runtime.rs) ─────┘
     runtime.rs:152   │  codegen.compile_for_runtime(&ast)
                      ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │  Stage 5: SCAN CYCLE LOOP (runtime.rs)                         │
 │                                                                │
 │  Variables stored as LLVM module-level globals (persist)       │
 │  __init_P() called once → stores initial values               │
 │  loop at fixed interval:                                       │
 │    1. call __scan_P()  → executes one scan cycle              │
 │    2. call __get_*()   → read each variable as f64            │
 │    3. update terminal dashboard                                │
 │    4. measure execution time + jitter                          │
 │    5. sleep until next cycle                                   │
 └─────────────────────────────────────────────────────────────────┘
```
 
---
 
## 3. Where the .st File is Read
 
**File: `src/main.rs`, lines 65–87** (compiler) and **`src/bin/runtime.rs`, lines 80–93** (runtime)
 
This is the single place where disk I/O happens in each binary. Everything downstream works on `&str` in memory.
 
```
Line 26:  let args: Vec<String> = env::args().collect();
            ↑ Command-line args. args[0] = "sdplc", args[1] = "programs/my_program.st"
 
Line 34:  let mut input_path: Option<String> = None;
            ↑ Will hold the path to the .st file
 
Line 58:  input_path = Some(args[i].clone());
            ↑ Any argument that isn't a flag gets treated as the input file
 
Line 66:  let (source_code, source_name) = match input_path {
Line 68:      let source = fs::read_to_string(path)
                ↑ THIS IS WHERE THE .st FILE IS READ INTO MEMORY.
                  The entire file becomes a single String.
 
Line 72:      let name = Path::new(path).file_stem()
                ↑ Extracts "my_program" from "programs/my_program.st"
                  Used as the output filename: my_program.ll, my_program.bc
 
Line 79:  None => { ... DEMO_PROGRAM.to_string() ... }
            ↑ If no file given, uses the hardcoded demo (line 226)
 
Line 89:  let out_base = output_name.unwrap_or(source_name.clone());
            ↑ The -o flag overrides the output name
```
 
After line 87, `source_code` is a `String` containing the raw ST source. This string is passed to the Lexer at line 100 and again at line 120.
 
**To change how input works** (e.g. reading from stdin, or accepting multiple files), you only need to modify lines 65–87 of `main.rs`. Nothing else in the compiler cares where the source came from.
 
---
 
## 4. Stage 1 — Lexer (`src/lexer.rs`)
 
**Purpose:** Convert raw source text into classified tokens.
 
**Entry point (from main.rs):**
 
```
Line 100:  let mut token_lexer = Lexer::new(&source_code);
Line 101:  let tokens = token_lexer.tokenize();
```
 
### Key Structures
 
|Structure|Line|Purpose|
|---|---|---|
|`enum TokenType`|16|Every possible token classification (80+ variants)|
|`struct Token`|282|One token: `{ kind, text, line, col }`|
|`struct Lexer`|319|Holds the source chars, position cursor, line/col counters|
 
### How the Lexer Works
 
The core loop is `next_token()` at **line 704**. Each call:
 
1. **Skips whitespace and comments** (line 386, `skip_whitespace_and_comments`)
    - Whitespace: any ASCII whitespace character
    - Block comments: `(* ... *)` with nesting support (depth counter)
    - Line comments: `// ... \n`
2. **Identifies what it's looking at** (lines 720–810):
    - Starts with `A-Z`, `a-z`, or `_` → identifier or keyword → calls `scan_identifier()` (line 442)
    - Starts with `0-9` → number → calls `scan_number()` (line 556)
    - Starts with `'` → string literal → calls `scan_string('\'')` (line 632)
    - Starts with `"` → wide string → calls `scan_string('"')` (line 632)
    - Two-character operators (`:=`, `<=`, `<>`, `**`, `..`, `->`, `=>`) checked first (lines 747–800)
    - Single-character operators fall through to the match at line 812
3. **Returns the Token** with kind, text, line, and column.
### The Keyword Table
 
**Lines 458–535** in `scan_identifier()`. After scanning an alphanumeric word, the text is uppercased and matched against this table. This is how `program` becomes `TokenType::Program` (case insensitive per IEC 61131-3).
 
**To add a new keyword:** Add a line to this match block and a new variant to `TokenType` (line 16).
 
### The Number Scanner
 
**Line 556, `scan_number()`**. Handles:
 
- Plain integers: `42`, `1_000` (underscores stripped later)
- Based integers: `16#FF`, `8#77`, `2#1010` — detects the `#` after digits
- Real literals: `3.14`, `1.0e-3` — detects `.` followed by digit (not `..`)
- The `..` vs `.` ambiguity: line 590 checks `peek() != Some('.')` before treating `.` as a decimal point. `0..9` correctly produces `IntLiteral(0), DotDot, IntLiteral(9)`.
### Temporal Literal Detection
 
**Lines 697–701** in `next_token()`. After scanning an identifier, if the next character is `#` and the identifier is one of `T`, `TIME`, `D`, `DATE`, `TOD`, `DT`, the lexer enters `scan_temporal_literal()` (line 671) to consume the value part: `T#5s`, `DATE#2025-12-01`, etc.
 
### `tokenize()` Convenience Method
 
**Line 833.** Calls `next_token()` in a loop until `Eof`, collecting everything into a `Vec<Token>`. This is what the parser consumes.
 
---
 
## 5. Stage 2 — Parser (`src/parser.rs`)
 
**Purpose:** Convert the token stream into an Abstract Syntax Tree.
 
**Entry point (from main.rs):**
 
```
Line 120:  let lexer = Lexer::new(&source_code);
Line 121:  let mut parser = Parser::new(lexer);
Line 122:  let ast = parser.parse()?;
```
 
### Key Structure
 
```
struct Parser {
    tokens: Vec<Token>,    // All tokens (consumed from Lexer at construction)
    pos: usize,            // Current read position
}
```
 
**Line 54:** `Parser::new(lexer)` immediately calls `lexer.tokenize()` to load all tokens into memory. The parser then walks `self.tokens[self.pos]` forward.
 
### Token Navigation Helpers (lines 60–100)
 
|Method|Line|Purpose|
|---|---|---|
|`current()`|60|Returns `&tokens[pos]` without advancing|
|`current_kind()`|66|Returns just the `TokenType` of current token|
|`advance()`|72|Returns current token and moves `pos` forward|
|`expect(kind)`|80|Consumes if kind matches, otherwise returns error|
|`eat(kind)`|92|Consumes if kind matches, returns `true`/`false`|
 
### Top-Level Parsing
 
**`parse()` at line 126.** Loops until EOF. Looks at the current token:
 
```
PROGRAM         → parse_program()      line 146
FUNCTION        → parse_function()     line 157
FUNCTION_BLOCK  → parse_function_block() line 170
anything else   → error
```
 
Each POU parser follows the same pattern: expect the opening keyword, expect the name (Ident), parse variable blocks, parse the statement body, expect the closing keyword.
 
### Variable Declaration Parsing
 
**`parse_var_blocks()` at line 183.** Loops while the current token starts a VAR section (VAR, VAR_INPUT, VAR_OUTPUT, etc.). For each:
 
```
parse_var_block()  line 202
  ↓
  expect VAR/VAR_INPUT/... keyword
  eat RETAIN?  eat CONSTANT?
  loop: parse_var_decl()  line 219
    ↓
    expect Ident (variable name)
    expect Colon
    parse_type_spec()  line 238
    if := then parse_expression() for initial value
    expect SemiColon
  expect END_VAR
```
 
**`parse_type_spec()` at line 238.** Maps token to type:
 
- `ARRAY` → `parse_array_type()` (expects `[lo..hi] OF type`)
- `STRING`/`WSTRING` → optional `[max_len]`
- Elementary type tokens (`INT`, `REAL`, `BOOL`, etc.) → `TypeSpec::Elementary`
- Any Ident → `TypeSpec::UserDefined` (resolved in semantic analysis)
### Statement Parsing
 
**`parse_statement()` at line 365.** Dispatches on the current token:
 
|Token|Calls|Line|
|---|---|---|
|`;`|returns `Statement::Empty`|367|
|`IF`|`parse_if()`|427|
|`FOR`|`parse_for()`|460|
|`WHILE`|`parse_while()`|484|
|`REPEAT`|`parse_repeat()`|496|
|`CASE`|`parse_case()`|508|
|`EXIT`|returns `Statement::Exit`|377|
|`RETURN`|returns `Statement::Return`|382|
|anything else|`parse_assignment_or_call()`|395|
 
**`parse_assignment_or_call()` at line 395** is the catch-all. It parses the left-hand side as an expression, then:
 
- If `:=` follows → it's an assignment, parse the RHS expression
- If `;` follows directly → it's a bare call statement
### Expression Parsing — Precedence Climbing
 
**Lines 593–604** define the precedence hierarchy. Each level calls the next-higher level:
 
```
parse_expression()  line 607  — entry point
  └→ parse_or_expr()        line 611   (OR)           ← lowest
       └→ parse_xor_expr()             (XOR)
            └→ parse_and_expr()        (AND, &)
                 └→ parse_equality_expr()   (=, <>)
                      └→ parse_comparison_expr() (<, >, <=, >=)
                           └→ parse_add_expr()     (+, -)
                                └→ parse_mul_expr()   (*, /, MOD)
                                     └→ parse_power_expr() (**)  ← right-assoc
                                          └→ parse_unary_expr()  (-, +, NOT)
                                               └→ parse_postfix_expr() ([i], .field)
                                                    └→ parse_primary()  ← highest
```
 
**`parse_primary()` at line 842** handles leaves:
 
- `IntLiteral` → parses the numeric value (handles base prefixes)
- `RealLiteral` → parses the f64 value
- `BoolLiteral` → checks `TRUE` vs `FALSE`
- `Ident` → if followed by `(` then it's a function call, otherwise an identifier
- `LParen` → parenthesized sub-expression
**`parse_postfix_expr()` at line 802** handles chained suffixes:
 
- `[` → array subscript
- `.` → member access (e.g. `fb_instance.output`)
**Why `2 + 3 * 4` parses as `2 + (3 * 4)`:** `parse_add_expr` calls `parse_mul_expr` for both left and right operands. So `*` binds tighter because it's resolved at a deeper call level before `+` gets a chance to consume it.
 
---
 
## 6. Stage 3 — Semantic Analysis (`src/semantic.rs`)
 
**Purpose:** Validate the AST and resolve types. Produces the `ProgramContext` that codegen consumes.
 
**Entry point (from main.rs):**
 
```
Line 137:  let sem_ctx = semantic::analyze(ast.clone());
```
 
**`analyze()` at line 1018** creates a `SemanticAnalyzer` and calls `self.analyze(ast)`.
 
### Key Structures
 
|Structure|Line|Purpose|
|---|---|---|
|`enum ResolvedType`|51|Every IEC 61131-3 type mapped to LLVM primitives|
|`struct SymbolInfo`|163|One variable: resolved type + qualifier + const/retain flags|
|`struct SymbolTable`|196|Scoped variable lookup (stack of HashMaps)|
|`struct Diagnostic`|271|One error or warning with message + location|
|`struct ProgramContext`|295|Output: AST + symbols + diagnostics|
|`struct SemanticAnalyzer`|318|Walks the AST, builds symbols, emits diagnostics|
 
### Type Resolution
 
**`resolve_type()` at line 382** maps AST `TypeSpec` → `ResolvedType`:
 
|TypeSpec|ResolvedType|LLVM|
|---|---|---|
|`Elementary(Int)`|`SignedInt { bits: 16 }`|`i16`|
|`Elementary(Dint)`|`SignedInt { bits: 32 }`|`i32`|
|`Elementary(Real)`|`Float { bits: 32 }`|`float`|
|`Elementary(Lreal)`|`Float { bits: 64 }`|`double`|
|`Elementary(Bool)`|`Bool`|`i1`|
|`Array { ranges, elem }`|`Array { element, ranges }`|`[N x elem]`|
|`StringType { max_len }`|`Str { max_len }`|`[N x i8]`|
|`UserDefined("MyFB")`|`UserDefined { name }`|`i64` placeholder|
 
**`resolve_elementary()` at line 409** is the core mapping table.
 
### Two-Pass Analysis
 
**Pass 1** (line 345): Register all POUs so forward references work. `register_pou()` records the POU's name, kind, return type, and parameter list in `SymbolTable.pous`.
 
**Pass 2** (line 350): For each POU, `analyze_pou()` at line 481:
 
1. Pushes a new scope on the symbol table
2. Calls `declare_var_blocks()` (line 518) to register every variable
3. Walks every statement via `analyze_statements()` → `analyze_statement()` (line 553)
4. Pops the scope
### What Gets Checked
 
**`analyze_statement()` at line 553** (the big switch):
 
|Statement|Checks|Error if violated|
|---|---|---|
|Assignment|Target exists, not CONSTANT, value type assignable to target type|"undeclared variable", "cannot assign to CONSTANT", "cannot assign X to Y"|
|IF|Condition must be BOOL|"IF condition must be BOOL, found INT"|
|FOR|Variable must be declared, integer type, not CONSTANT; bounds numeric|"FOR variable must be an integer type"|
|WHILE|Condition must be BOOL|"WHILE condition must be BOOL"|
|REPEAT/UNTIL|UNTIL condition must be BOOL|"UNTIL condition must be BOOL"|
|CASE|Selector must be integer|"CASE selector must be integer type"|
|EXIT|Must be inside a loop (tracked by `loop_depth`)|"EXIT is only valid inside a loop"|
 
**`check_expression()` at line 699** recursively types every expression:
 
- Literal → known type (IntLiteral → `SignedInt{32}`, RealLiteral → `Float{64}`)
- Identifier → look up in symbol table, return its resolved type (or error if undeclared)
- BinaryOp → check both sides, then `check_binary_op()` (line 827) validates operator compatibility and returns the result type
- UnaryOp → check operand type
- ArrayAccess → check index is integer, return element type
- FunctionCall → look up POU, return its return type
**`promote_numeric()` at line 898:** When two numeric operands meet, decides the result type. Float wins over int, wider wins over narrower, signed wins over unsigned.
 
**`check_assignable()` at line 928:** Can you store type A into variable of type B? Same type always works. Numeric-to-numeric is allowed (implicit widening). Bool-to-Int is an error.
 
---
 
## 7. Stage 4 — Code Generation (`src/codegen.rs`)
 
**Purpose:** Walk the AST and emit LLVM IR via inkwell.
 
**Entry point (from main.rs):**
 
```
Line 155:  let llvm_context = Context::create();
Line 156:  let mut codegen = CodeGenerator::new(&llvm_context, &source_name);
Line 158:  codegen.compile(&ast)?;
```
 
### Key Structure
 
```rust
pub struct CodeGenerator<'ctx> {
    context: &'ctx Context,          // LLVM context (types, constants)
    module: Module<'ctx>,            // LLVM module (functions, globals)
    builder: Builder<'ctx>,          // LLVM IR builder (instructions)
    current_fn: Option<FunctionValue>,// Currently emitting function
    variables: HashMap<String, VarSlot>, // variable name → alloca ptr + type
    functions: HashMap<String, FunctionValue>, // POU name → LLVM function
    loop_exit_stack: Vec<BasicBlock>, // EXIT targets (innermost loop exits)
}
```
 
### Type Mapping
 
**`llvm_type()` at line 194** converts `ResolvedType` → inkwell `BasicTypeEnum`:
 
```
Bool              → context.bool_type()           → i1
SignedInt{16}     → context.custom_width_int_type(16) → i16
Float{32}        → context.f32_type()             → float
Float{64}        → context.f64_type()             → double
Time/Date/Tod/Dt  → context.i64_type()            → i64
Array{elem, [0..9]} → elem_type.array_type(10)    → [10 x elem]
Str{254}          → i8_type.array_type(255)        → [255 x i8]
```
 
### Two-Pass Compilation
 
**`compile()` at line 256.**
 
**Pass 1 — `declare_pou()`** (line 269): Creates LLVM function signatures without bodies.
 
- `PROGRAM P` → `define void @P()`
- `FUNCTION Add : INT` with `VAR_INPUT a, b : INT` → `define i16 @Add(i16 %a, i16 %b)`
- `FUNCTION_BLOCK FB` → `define void @FB()`
**Pass 2 — `emit_pou()`** (line 320): For each POU:
 
1. Creates the `entry` basic block
2. Positions the builder at the entry block
3. Emits `alloca` for each variable via `emit_var_decl()` (line 450)
4. Stores initial values (or zero)
5. Walks statements via `emit_statements()` → `emit_statement()` (line 517)
6. Adds `ret void` or `ret <value>` terminator
### Statement Emission
 
**`emit_statement()` at line 517** dispatches to specialised methods:
 
|Statement|Method|Line|LLVM Pattern|
|---|---|---|---|
|Assignment|`emit_assignment()`|575|`store` to alloca (or GEP for arrays)|
|IF|`emit_if()`|601|`br i1 %cond, label %then, label %else`|
|FOR|`emit_for()`|672|4 blocks: cond → body → inc → exit|
|WHILE|`emit_while()`|764|3 blocks: cond → body → exit|
|REPEAT|`emit_repeat()`|795|3 blocks: body → cond → exit|
|CASE|`emit_case()`|827|chain of icmp + br blocks|
|EXIT|unconditional branch|561|`br label %loop.exit`|
|RETURN|loads return slot, returns|565|`ret i16 %retval`|
 
### Expression Emission
 
**`emit_expression()` at line 914** returns a `BasicValueEnum` (an SSA value):
 
|Expression|LLVM Result|
|---|---|
|`IntLiteral { value: 42 }`|`i32 42`|
|`RealLiteral { value: 3.14 }`|`double 3.14`|
|`BoolLiteral { value: true }`|`i1 1`|
|`Identifier { name: "speed" }`|`%speed = load float, float* %speed.addr`|
|`BinaryOp { Add, left, right }`|`%add = fadd float %left, %right`|
|`UnaryOp { Neg, operand }`|`%neg = fneg float %operand`|
|`FunctionCall { "Add", args }`|`%call = call i16 @Add(i16 %a, i16 %b)`|
|`ArrayAccess { "arr", [i] }`|`%gep = getelementptr [10 x i32], ... ; %val = load i32, ...`|
 
### Coercion Helpers
 
|Helper|Line|Purpose|
|---|---|---|
|`to_bool()`|1197|Convert any value to `i1` (int: compare != 0, float: fcmp ONE 0.0)|
|`to_f64()`|1216|Convert int to float (`sitofp`), or extend f32 to f64|
|`coerce_int()`|1232|Sign-extend or truncate integer to target width|
|`coerce_value()`|1246|General coercion: int↔float, narrow↔wide|
|`zero_value()`|475|Default initialiser for a type (0, 0.0, false, etc.)|
 
### Output
 
**`write_ir()` at line 130:** Writes human-readable `.ll` file. **`write_bitcode()` at line 136:** Writes binary `.bc` file. **`ir_string()` at line 125:** Returns the IR as a Rust `String` (used by `--emit-ir`).
 
---
 
## 8. Stage 5 — Runtime Execution (`src/bin/runtime.rs`)
 
**Purpose:** JIT-compile and execute an ST program in a deterministic scan cycle loop with live variable display.
 
**Entry point:**
 
```bash
cargo run --bin runtime -- programs/my_program.st --scan-time=100
```
 
### How it Differs from the Compiler
 
The compiler (`main.rs`) calls `codegen.compile(&ast)`, which stores variables as **stack allocas**. Each call to the generated function creates fresh variables — there's no state between calls.
 
The runtime calls `codegen.compile_for_runtime(&ast)` (codegen.rs line 1314), which stores variables as **LLVM module-level globals**. These persist across calls, so each scan cycle sees the values from the previous cycle.
 
### What `compile_for_runtime()` Emits
 
For a `PROGRAM ConveyorControl` with variables `speed`, `count`, `running`:
 
```llvm
; Global variables (persist across scan cycles)
@speed   = global float 0.0
@count   = global i16 0
@running = global i1 false
 
; Called once at startup — stores initial values
define void @__init_ConveyorControl() { ... }
 
; Called every scan cycle — executes the program body
define void @__scan_ConveyorControl() { ... }
 
; Getters — read each variable as f64 for uniform display
define double @__get_speed() { ... }
define double @__get_count() { ... }
define double @__get_running() { ... }
```
 
### Runtime Loop
 
```
1. Create LLVM ExecutionEngine with JIT (runtime.rs line 171)
2. Get JitFunction pointers: __init, __scan, __get_* (lines 178–200)
3. Call __init once (line 206)
4. Loop:
   a. Record cycle start time
   b. Call __scan  → executes one scan cycle
   c. Call each __get_* → read variable values
   d. Clear terminal, print dashboard
   e. Measure jitter
   f. Sleep until next scan interval
5. On exit: print summary with cycle count, avg/max jitter, final values
```
 
### Key Structures in runtime.rs
 
|Item|Line|Purpose|
|---|---|---|
|`type VoidFn`|33|JIT function signature for `__init` and `__scan`|
|`type GetterFn`|35|JIT function signature for `__get_*` → returns f64|
|`scan_time_ms`|50|Scan cycle interval from `--scan-time=` flag|
|`max_cycles`|51|Optional stop count from `--cycles=` flag|
|`jitter_sum / jitter_max`|212–213|Timing measurements for determinism evidence|
 
### RuntimeVar (codegen.rs line 83)
 
```rust
pub struct RuntimeVar {
    pub name: String,           // "speed"
    pub resolved_type: ResolvedType, // Float { bits: 32 }
    pub program_name: String,   // "ConveyorControl"
}
```
 
The runtime uses this to:
 
- Know which `__get_*` functions to call
- Format values correctly (BOOL → "TRUE"/"FALSE", INT → integer, REAL → decimal)
- Display the type name in the dashboard
---
 
## 9. The AST — Shared Data Model (`src/ast.rs`)
 
The AST is the contract between the parser (producer) and semantic analysis + codegen (consumers). All three graphical languages would also produce this same AST.
 
### Hierarchy
 
```
CompilationUnit                         line 25
  └→ Vec<Pou>                           line 31
       ├→ Program  { name, var_blocks, body, span }       line 39
       ├→ Function { name, return_type, var_blocks, body } line 48
       └→ FunctionBlock { name, var_blocks, body }         line 58
 
VarBlock { qualifier, retain, constant, declarations }     line 88
  └→ Vec<VarDecl>                       line 98
       └→ VarDecl { name, type_spec, initial_value }
 
TypeSpec                                line 109
  ├→ Elementary(ElementaryType)         line 127
  ├→ Array { ranges, element_type }
  ├→ StringType { max_len }
  ├→ WStringType { max_len }
  └→ UserDefined(String)
 
Statement                               line 160
  ├→ Assignment { target, value }
  ├→ If { condition, then_body, elsif_branches, else_body }
  ├→ For { variable, from, to, by, body }
  ├→ While { condition, body }
  ├→ Repeat { body, condition }
  ├→ Case { selector, branches, else_body }
  ├→ Exit
  ├→ Return
  ├→ CallStatement { name, args }
  └→ Empty
 
Expression                              line 248
  ├→ IntLiteral { value: i64 }
  ├→ RealLiteral { value: f64 }
  ├→ BoolLiteral { value: bool }
  ├→ StringLiteral / WStringLiteral
  ├→ TimeLiteral / DateLiteral / TodLiteral / DtLiteral
  ├→ Identifier { name }
  ├→ BinaryOp { left, op, right }
  ├→ UnaryOp { op, operand }
  ├→ FunctionCall { name, args }
  ├→ ArrayAccess { array, indices }
  └→ MemberAccess { object, member }
```
 
Every node carries a `Span { line, col }` for error reporting.
 
---
 
## 10. The Crate Root (`src/lib.rs`)
 
**51 lines.** Simply declares the five modules as public:
 
```rust
pub mod ast;
pub mod codegen;
pub mod lexer;
pub mod parser;
pub mod semantic;
```
 
This makes them importable by:
 
- `main.rs` (the compiler binary) via `use sdplc::lexer::Lexer;`
- `bin/runtime.rs` (the runtime binary) via `use sdplc::codegen::CodeGenerator;`
- Integration tests in `tests/` via `use sdplc::codegen::CodeGenerator;`
- External crates that depend on `sdplc` as a library
---
 
## 11. Test Structure
 
### Unit Tests (inside `src/*.rs`)
 
Each module has a `#[cfg(test)] mod tests { ... }` block at the bottom.
 
|Module|Count|Tests|
|---|---|---|
|`lexer.rs`|29|Token classification, comments, literals, operators, line tracking|
|`parser.rs`|22|AST structure, precedence, control flow, error cases|
|`semantic.rs`|25|Type resolution, variable scoping, type errors, promotions|
|`codegen.rs`|14|IR output contains expected instructions and blocks|
 
Run unit tests only: `cargo test --lib`
 
### Integration Tests (`tests/`)
 
These test the full pipeline from source string to output.
 
|File|Count|Tests|
|---|---|---|
|`lexer_integration_test.rs`|10|Full programs lex without unknowns|
|`parser_integration_test.rs`|8|Full programs parse to correct AST structure|
|`semantic_integration_test.rs`|18|Type errors caught, valid programs pass clean|
|`codegen_integration_test.rs`|11|LLVM IR contains expected instructions|
 
Run integration tests only: `cargo test --test codegen_integration_test`
 
Run everything: `cargo test`
 
---
 
## 12. Common Modifications
 
### "I want to add a new IEC 61131-3 keyword"
 
1. Add variant to `TokenType` in `lexer.rs` line 16
2. Add the keyword string to the match table in `lexer.rs` line 458
3. Write a test in the `mod tests` block
### "I want to add a new data type"
 
1. Add variant to `ElementaryType` in `ast.rs` line 127
2. Add to `Display` impl in `ast.rs` line 337
3. Add token to `TokenType` in `lexer.rs`, add to keyword table
4. Add to `try_elementary_type()` in `parser.rs`
5. Add to `resolve_elementary()` in `semantic.rs` line 409
6. Add to `llvm_type()` in `codegen.rs` line 194
### "I want to add a new statement type"
 
1. Add variant to `Statement` in `ast.rs` line 160
2. Add parsing branch in `parse_statement()` in `parser.rs` line 365
3. Add checking in `analyze_statement()` in `semantic.rs` line 553
4. Add emission in `emit_statement()` in `codegen.rs` line 517
### "I want to change how files are loaded"
 
Edit `main.rs` lines 65–87 only. The rest of the compiler works on `&str`.
 
### "I want to add PLCopen XML input"
 
1. Create `src/plcopen.rs` — parse XML, produce `CompilationUnit`
2. Add `pub mod plcopen;` to `lib.rs`
3. In `main.rs`, detect `.xml` extension and route to the XML parser instead of `Lexer` + `Parser`
4. Everything from semantic analysis onward works unchanged
### "I want to target a new architecture"
 
No code changes needed. Just use `llc` with the right triple:
 
```bash
llc output.ll -mtriple=riscv64-linux-gnu -o output_riscv.s
```
 
LLVM handles the backend. The SD-PLC compiler only produces the architecture-independent IR.
 
### "I want to change the scan cycle timing"
 
Edit the `--scan-time` flag when running the runtime. The sleep logic is in `runtime.rs` line 256. For sub-millisecond cycles, replace `std::thread::sleep` with a spin-wait loop — see the `spin_sleep` crate.
 
### "I want to add a variable to the runtime dashboard"
 
Variables are automatically exposed. Any variable declared in a PROGRAM's VAR block gets a `__get_*` getter emitted by `compile_program_for_runtime()` (codegen.rs line 1359). The runtime discovers them via the `Vec<RuntimeVar>` returned by `compile_for_runtime()`.
 
### "I want to log scan cycle data to CSV"
 
In `runtime.rs`, after the `values` vector is populated (line 222), write a line to a file:
 
```rust
writeln!(csv_file, "{},{}", cycle, values.iter()
    .map(|v| v.to_string()).collect::<Vec<_>>().join(",")).ok();
```
 
This gives you publication-quality data for plotting in Python and comparing against CODESYS traces.
