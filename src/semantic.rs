//! Semantic analysis for IEC 61131-3 Structured Text.
//!
//! This module validates a parsed AST and produces a [`ProgramContext`]
//! containing resolved type information and a symbol table suitable for
//! direct consumption by LLVM IR generation via `inkwell`.
//!
//! ## LLVM Type Mapping
//!
//! | IEC 61131-3 | LLVM IR | Bits |
//! |-------------|---------|------|
//! | BOOL        | i1      | 1    |
//! | SINT        | i8      | 8    |
//! | INT         | i16     | 16   |
//! | DINT        | i32     | 32   |
//! | LINT        | i64     | 64   |
//! | USINT       | i8      | 8    |
//! | UINT        | i16     | 16   |
//! | UDINT       | i32     | 32   |
//! | ULINT       | i64     | 64   |
//! | REAL        | float   | 32   |
//! | LREAL       | double  | 64   |
//! | BYTE        | i8      | 8    |
//! | WORD        | i16     | 16   |
//! | DWORD       | i32     | 32   |
//! | LWORD       | i64     | 64   |

use crate::ast::*;
use std::collections::HashMap;

// ─── Resolved Types ─────────────────────────────────────────────

/// A fully resolved type with known LLVM mapping.
///
/// This is the bridge between the AST's [`TypeSpec`] and inkwell's
/// type constructors. Codegen matches on this to call the correct
/// `context.i32_type()`, `context.f64_type()`, etc.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedType {
    /// A boolean — LLVM `i1`.
    Bool,
    /// A signed integer with known bit width.
    SignedInt { bits: u32 },
    /// An unsigned integer with known bit width.
    UnsignedInt { bits: u32 },
    /// A floating-point number — 32-bit (REAL) or 64-bit (LREAL).
    Float { bits: u32 },
    /// A bit string (BYTE/WORD/DWORD/LWORD) — LLVM integer.
    BitString { bits: u32 },
    /// A duration (TIME) — stored as i64 milliseconds, matching the
    /// resolution of the runtime scan clock read by `TIME_MS()`.
    Time,
    /// A date (DATE) — stored as i64 days since epoch.
    Date,
    /// A time-of-day (TOD) — stored as i64 nanoseconds since midnight.
    Tod,
    /// A date-and-time (DT) — stored as i64 nanoseconds since epoch.
    Dt,
    /// A fixed-length string — LLVM `[N x i8]`.
    Str { max_len: u32 },
    /// A wide string — LLVM `[N x i16]`.
    WStr { max_len: u32 },
    /// An array type.
    Array {
        element: Box<ResolvedType>,
        ranges: Vec<ArrayRange>,
    },
    /// A user-defined type (function block instance, struct, etc.).
    UserDefined { name: String },
    /// Represents a type that could not be resolved (error recovery).
    Error,
}

impl ResolvedType {
    /// Returns the LLVM integer bit width, or `None` for non-integer types.
    pub fn int_bits(&self) -> Option<u32> {
        match self {
            Self::Bool => Some(1),
            Self::SignedInt { bits } | Self::UnsignedInt { bits } | Self::BitString { bits } => {
                Some(*bits)
            }
            _ => None,
        }
    }

    /// Returns true if this is a numeric type (integer or float).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::SignedInt { .. }
                | Self::UnsignedInt { .. }
                | Self::Float { .. }
                | Self::BitString { .. }
        )
    }

    /// Returns true if this is an integer type (signed, unsigned, or bit string).
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::SignedInt { .. } | Self::UnsignedInt { .. } | Self::BitString { .. }
        )
    }

    /// Returns true if this is a floating-point type.
    pub fn is_float(&self) -> bool {
        matches!(self, Self::Float { .. })
    }

    /// Returns true if this type is signed.
    pub fn is_signed(&self) -> bool {
        matches!(self, Self::SignedInt { .. } | Self::Float { .. })
    }

    /// Returns true for the temporal types, all of which are stored as
    /// signed 64-bit counts.
    pub fn is_temporal(&self) -> bool {
        matches!(self, Self::Time | Self::Date | Self::Tod | Self::Dt)
    }
}

impl std::fmt::Display for ResolvedType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool => write!(f, "BOOL"),
            Self::SignedInt { bits } => match bits {
                8 => write!(f, "SINT"),
                16 => write!(f, "INT"),
                32 => write!(f, "DINT"),
                64 => write!(f, "LINT"),
                _ => write!(f, "i{}", bits),
            },
            Self::UnsignedInt { bits } => match bits {
                8 => write!(f, "USINT"),
                16 => write!(f, "UINT"),
                32 => write!(f, "UDINT"),
                64 => write!(f, "ULINT"),
                _ => write!(f, "u{}", bits),
            },
            Self::Float { bits } => match bits {
                32 => write!(f, "REAL"),
                64 => write!(f, "LREAL"),
                _ => write!(f, "f{}", bits),
            },
            Self::BitString { bits } => match bits {
                8 => write!(f, "BYTE"),
                16 => write!(f, "WORD"),
                32 => write!(f, "DWORD"),
                64 => write!(f, "LWORD"),
                _ => write!(f, "bits{}", bits),
            },
            Self::Time => write!(f, "TIME"),
            Self::Date => write!(f, "DATE"),
            Self::Tod => write!(f, "TOD"),
            Self::Dt => write!(f, "DT"),
            Self::Str { max_len } => write!(f, "STRING[{}]", max_len),
            Self::WStr { max_len } => write!(f, "WSTRING[{}]", max_len),
            Self::Array { element, .. } => write!(f, "ARRAY OF {}", element),
            Self::UserDefined { name } => write!(f, "{}", name),
            Self::Error => write!(f, "<error>"),
        }
    }
}

// ─── Symbol Table ───────────────────────────────────────────────

/// Metadata for a declared symbol (variable, parameter, etc.).
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    /// The resolved type of this symbol.
    pub resolved_type: ResolvedType,
    /// The original type specifier from the AST.
    pub type_spec: TypeSpec,
    /// The variable qualifier (VAR, VAR_INPUT, etc.).
    pub qualifier: VarQualifier,
    /// Whether this variable is declared CONSTANT.
    pub is_constant: bool,
    /// Whether this variable is declared RETAIN.
    pub is_retain: bool,
    /// Source location of the declaration.
    pub span: Span,
}

/// Information about a declared POU (for resolving calls).
#[derive(Debug, Clone)]
pub struct PouInfo {
    pub kind: PouKind,
    pub return_type: Option<ResolvedType>,
    pub parameters: Vec<(String, ResolvedType, VarQualifier)>,
}

/// What kind of POU this is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PouKind {
    Program,
    Function,
    FunctionBlock,
}

/// A scoped symbol table with support for nested scopes.
#[derive(Debug)]
pub struct SymbolTable {
    /// Stack of scopes — the last is the current (innermost).
    scopes: Vec<HashMap<String, SymbolInfo>>,
    /// Registered POUs for resolving function/FB calls.
    pous: HashMap<String, PouInfo>,
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable {
            scopes: vec![HashMap::new()], // global scope
            pous: HashMap::new(),
        }
    }

    /// Opens a new scope (e.g. entering a POU body).
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Closes the current scope.
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Declares a symbol in the current (innermost) scope.
    /// Returns an error message if the symbol is already declared
    /// in this scope.
    pub fn declare(&mut self, name: &str, info: SymbolInfo) -> Option<String> {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            Some(format!("'{}' is already declared in this scope", name))
        } else {
            scope.insert(name.to_string(), info);
            None
        }
    }

    /// Looks up a symbol by name, searching from innermost to outermost scope.
    pub fn lookup(&self, name: &str) -> Option<&SymbolInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    /// Registers a POU for call resolution.
    pub fn register_pou(&mut self, name: &str, info: PouInfo) {
        self.pous.insert(name.to_string(), info);
    }

    /// Looks up a registered POU.
    pub fn lookup_pou(&self, name: &str) -> Option<&PouInfo> {
        self.pous.get(name)
    }

    /// Returns all symbols in the current scope (for codegen to allocate).
    pub fn current_scope_symbols(&self) -> &HashMap<String, SymbolInfo> {
        self.scopes.last().unwrap()
    }
}

// ─── Diagnostics ────────────────────────────────────────────────

/// Severity level for a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

/// A diagnostic message produced during semantic analysis.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(
            f,
            "[{}:{}] {}: {}",
            self.line, self.col, prefix, self.message
        )
    }
}

// ─── Program Context (output of semantic analysis) ──────────────

/// The validated output of semantic analysis, consumed by codegen.
///
/// Contains the original AST, the symbol table with resolved types,
/// and any diagnostics.
pub struct ProgramContext {
    pub ast: CompilationUnit,
    pub symbols: SymbolTable,
    pub diagnostics: Vec<Diagnostic>,
}

impl ProgramContext {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }
}

// ─── Semantic Analyser ──────────────────────────────────────────

/// Walks the AST, validates semantics, and produces a [`ProgramContext`].
pub struct SemanticAnalyzer {
    symbols: SymbolTable,
    diagnostics: Vec<Diagnostic>,
    /// Current loop nesting depth (for EXIT validation).
    loop_depth: usize,
    /// Whether we are inside a FUNCTION (for RETURN type checking).
    in_function: bool,
    /// The return type of the current function (if any).
    current_return_type: Option<ResolvedType>,
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        SemanticAnalyzer {
            symbols: SymbolTable::new(),
            diagnostics: Vec::new(),
            loop_depth: 0,
            in_function: false,
            current_return_type: None,
        }
    }

    /// Analyses a compilation unit and returns the program context.
    pub fn analyze(mut self, ast: CompilationUnit) -> ProgramContext {
        // First pass: register all POUs so forward references work.
        //
        // The standard function blocks the program instantiates are
        // registered alongside them — that is what makes `t : TON;`
        // resolve and `t.Q` have a known type. Their bodies are not
        // re-validated on every build; this module's tests cover them.
        let with_library = crate::stdlib::inject(&ast);
        for pou in &with_library.units {
            self.register_pou(pou);
        }

        // Second pass: validate each POU body.
        for pou in &ast.units {
            self.analyze_pou(pou);
        }

        ProgramContext {
            ast,
            symbols: self.symbols,
            diagnostics: self.diagnostics,
        }
    }

    // ── Helpers ──────────────────────────────────────────────────

    fn error(&mut self, span: &Span, message: String) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message,
            line: span.line,
            col: span.col,
        });
    }

    fn warning(&mut self, span: &Span, message: String) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            message,
            line: span.line,
            col: span.col,
        });
    }

    // ── Type resolution ─────────────────────────────────────────

    /// Resolves an AST [`TypeSpec`] to a [`ResolvedType`].
    pub fn resolve_type(&mut self, ts: &TypeSpec) -> ResolvedType {
        match ts {
            TypeSpec::Elementary(et) => self.resolve_elementary(et),
            TypeSpec::Array {
                ranges,
                element_type,
            } => {
                let element = self.resolve_type(element_type);
                ResolvedType::Array {
                    element: Box::new(element),
                    ranges: ranges.clone(),
                }
            }
            TypeSpec::StringType { max_len } => ResolvedType::Str {
                max_len: max_len.unwrap_or(254),
            },
            TypeSpec::WStringType { max_len } => ResolvedType::WStr {
                max_len: max_len.unwrap_or(254),
            },
            TypeSpec::UserDefined(name) => {
                // Check if it's a known function block
                if self.symbols.lookup_pou(name).is_some() {
                    ResolvedType::UserDefined { name: name.clone() }
                } else {
                    ResolvedType::UserDefined { name: name.clone() }
                }
            }
        }
    }

    fn resolve_elementary(&self, et: &ElementaryType) -> ResolvedType {
        match et {
            ElementaryType::Bool => ResolvedType::Bool,
            ElementaryType::Sint => ResolvedType::SignedInt { bits: 8 },
            ElementaryType::Int => ResolvedType::SignedInt { bits: 16 },
            ElementaryType::Dint => ResolvedType::SignedInt { bits: 32 },
            ElementaryType::Lint => ResolvedType::SignedInt { bits: 64 },
            ElementaryType::Usint => ResolvedType::UnsignedInt { bits: 8 },
            ElementaryType::Uint => ResolvedType::UnsignedInt { bits: 16 },
            ElementaryType::Udint => ResolvedType::UnsignedInt { bits: 32 },
            ElementaryType::Ulint => ResolvedType::UnsignedInt { bits: 64 },
            ElementaryType::Real => ResolvedType::Float { bits: 32 },
            ElementaryType::Lreal => ResolvedType::Float { bits: 64 },
            ElementaryType::Byte => ResolvedType::BitString { bits: 8 },
            ElementaryType::Word => ResolvedType::BitString { bits: 16 },
            ElementaryType::Dword => ResolvedType::BitString { bits: 32 },
            ElementaryType::Lword => ResolvedType::BitString { bits: 64 },
            ElementaryType::Time => ResolvedType::Time,
            ElementaryType::Date => ResolvedType::Date,
            ElementaryType::Tod => ResolvedType::Tod,
            ElementaryType::Dt => ResolvedType::Dt,
        }
    }

    // ── POU registration (pass 1) ───────────────────────────────

    fn register_pou(&mut self, pou: &Pou) {
        match pou {
            Pou::Program(p) => {
                self.symbols.register_pou(
                    &p.name,
                    PouInfo {
                        kind: PouKind::Program,
                        return_type: None,
                        parameters: Vec::new(),
                    },
                );
            }
            Pou::Function(f) => {
                let ret = self.resolve_type(&f.return_type);
                let params = self.extract_parameters(&f.var_blocks);
                self.symbols.register_pou(
                    &f.name,
                    PouInfo {
                        kind: PouKind::Function,
                        return_type: Some(ret),
                        parameters: params,
                    },
                );
            }
            Pou::FunctionBlock(fb) => {
                let params = self.extract_parameters(&fb.var_blocks);
                self.symbols.register_pou(
                    &fb.name,
                    PouInfo {
                        kind: PouKind::FunctionBlock,
                        return_type: None,
                        parameters: params,
                    },
                );
            }
        }
    }

    fn extract_parameters(
        &mut self,
        var_blocks: &[VarBlock],
    ) -> Vec<(String, ResolvedType, VarQualifier)> {
        let mut params = Vec::new();
        for block in var_blocks {
            if matches!(
                block.qualifier,
                VarQualifier::VarInput | VarQualifier::VarOutput | VarQualifier::VarInOut
            ) {
                for decl in &block.declarations {
                    let rt = self.resolve_type(&decl.type_spec);
                    params.push((decl.name.clone(), rt, block.qualifier.clone()));
                }
            }
        }
        params
    }

    // ── POU analysis (pass 2) ───────────────────────────────────

    fn analyze_pou(&mut self, pou: &Pou) {
        match pou {
            Pou::Program(p) => {
                self.symbols.push_scope();
                self.declare_var_blocks(&p.var_blocks);
                self.analyze_statements(&p.body);
                self.symbols.pop_scope();
            }
            Pou::Function(f) => {
                self.symbols.push_scope();
                let ret = self.resolve_type(&f.return_type);
                // Declare the function name as a variable for return value assignment
                self.symbols.declare(
                    &f.name,
                    SymbolInfo {
                        resolved_type: ret.clone(),
                        type_spec: f.return_type.clone(),
                        qualifier: VarQualifier::Var,
                        is_constant: false,
                        is_retain: false,
                        span: f.span.clone(),
                    },
                );
                self.declare_var_blocks(&f.var_blocks);
                self.in_function = true;
                self.current_return_type = Some(ret);
                self.analyze_statements(&f.body);
                self.in_function = false;
                self.current_return_type = None;
                self.symbols.pop_scope();
            }
            Pou::FunctionBlock(fb) => {
                self.symbols.push_scope();
                self.declare_var_blocks(&fb.var_blocks);
                self.analyze_statements(&fb.body);
                self.symbols.pop_scope();
            }
        }
    }

    fn declare_var_blocks(&mut self, blocks: &[VarBlock]) {
        for block in blocks {
            for decl in &block.declarations {
                let resolved = self.resolve_type(&decl.type_spec);

                // Validate initial value type if present
                if let Some(ref init_expr) = decl.initial_value {
                    let init_type = self.check_expression(init_expr);
                    if init_type != ResolvedType::Error && resolved != ResolvedType::Error {
                        self.check_assignable(&resolved, &init_type, &decl.span);
                    }
                }

                if let Some(msg) = self.symbols.declare(
                    &decl.name,
                    SymbolInfo {
                        resolved_type: resolved,
                        type_spec: decl.type_spec.clone(),
                        qualifier: block.qualifier.clone(),
                        is_constant: block.constant,
                        is_retain: block.retain,
                        span: decl.span.clone(),
                    },
                ) {
                    self.error(&decl.span, msg);
                }
            }
        }
    }

    // ── Statement analysis ──────────────────────────────────────

    fn analyze_statements(&mut self, stmts: &[Statement]) {
        for stmt in stmts {
            self.analyze_statement(stmt);
        }
    }

    fn analyze_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Assignment {
                target,
                value,
                span,
            } => {
                let target_type = self.check_expression(target);
                let value_type = self.check_expression(value);

                // Check that target is assignable (not CONSTANT)
                if let Expression::Identifier { name, .. } = target {
                    if let Some(sym) = self.symbols.lookup(name) {
                        if sym.is_constant {
                            self.error(span, format!("cannot assign to CONSTANT '{}'", name));
                        }
                    }
                }

                if target_type != ResolvedType::Error && value_type != ResolvedType::Error {
                    self.check_assignable(&target_type, &value_type, span);
                }
            }

            Statement::If {
                condition,
                then_body,
                elsif_branches,
                else_body,
                span,
            } => {
                let cond_type = self.check_expression(condition);
                self.require_bool(&cond_type, span, "IF condition");

                self.analyze_statements(then_body);

                for (cond, body) in elsif_branches {
                    let ct = self.check_expression(cond);
                    self.require_bool(&ct, span, "ELSIF condition");
                    self.analyze_statements(body);
                }

                if let Some(body) = else_body {
                    self.analyze_statements(body);
                }
            }

            Statement::For {
                variable,
                from,
                to,
                by,
                body,
                span,
            } => {
                // FOR variable must be declared and integer
                if let Some(sym) = self.symbols.lookup(variable).cloned() {
                    if !sym.resolved_type.is_integer() {
                        self.error(
                            span,
                            format!(
                                "FOR variable '{}' must be an integer type, found {}",
                                variable, sym.resolved_type
                            ),
                        );
                    }
                    if sym.is_constant {
                        self.error(
                            span,
                            format!("cannot use CONSTANT '{}' as FOR variable", variable),
                        );
                    }
                } else {
                    self.error(
                        span,
                        format!("undeclared variable '{}' in FOR loop", variable),
                    );
                }

                let from_type = self.check_expression(from);
                let to_type = self.check_expression(to);
                self.require_numeric(&from_type, span, "FOR lower bound");
                self.require_numeric(&to_type, span, "FOR upper bound");

                if let Some(by_expr) = by {
                    let by_type = self.check_expression(by_expr);
                    self.require_numeric(&by_type, span, "FOR step");
                }

                self.loop_depth += 1;
                self.analyze_statements(body);
                self.loop_depth -= 1;
            }

            Statement::While {
                condition,
                body,
                span,
            } => {
                let cond_type = self.check_expression(condition);
                self.require_bool(&cond_type, span, "WHILE condition");

                self.loop_depth += 1;
                self.analyze_statements(body);
                self.loop_depth -= 1;
            }

            Statement::Repeat {
                body,
                condition,
                span,
            } => {
                self.loop_depth += 1;
                self.analyze_statements(body);
                self.loop_depth -= 1;

                let cond_type = self.check_expression(condition);
                self.require_bool(&cond_type, span, "UNTIL condition");
            }

            Statement::Case {
                selector,
                branches,
                else_body,
                span,
            } => {
                let sel_type = self.check_expression(selector);
                if sel_type != ResolvedType::Error && !sel_type.is_integer() {
                    self.error(
                        span,
                        format!("CASE selector must be integer type, found {}", sel_type),
                    );
                }

                for branch in branches {
                    for label in &branch.labels {
                        match label {
                            CaseLabel::Value(expr) => {
                                self.check_expression(expr);
                            }
                            CaseLabel::Range(lo, hi) => {
                                self.check_expression(lo);
                                self.check_expression(hi);
                            }
                        }
                    }
                    self.analyze_statements(&branch.body);
                }

                if let Some(body) = else_body {
                    self.analyze_statements(body);
                }
            }

            Statement::Exit { span } => {
                if self.loop_depth == 0 {
                    self.error(span, "EXIT is only valid inside a loop".to_string());
                }
            }

            Statement::Return { span } => {
                if !self.in_function {
                    self.warning(span, "RETURN outside of FUNCTION has no effect".to_string());
                }
            }

            Statement::CallStatement { name, args, span } => {
                self.check_call(name, args, span);
            }

            Statement::Empty => {}
        }
    }

    // ── Expression type checking ────────────────────────────────

    /// Determines the type of an expression, emitting diagnostics
    /// for any type errors found.
    pub fn check_expression(&mut self, expr: &Expression) -> ResolvedType {
        match expr {
            Expression::IntLiteral { .. } => ResolvedType::SignedInt { bits: 32 },
            Expression::RealLiteral { .. } => ResolvedType::Float { bits: 64 },
            Expression::BoolLiteral { .. } => ResolvedType::Bool,
            Expression::StringLiteral { .. } => ResolvedType::Str { max_len: 254 },
            Expression::WStringLiteral { .. } => ResolvedType::WStr { max_len: 254 },
            Expression::TimeLiteral { .. } => ResolvedType::Time,
            Expression::DateLiteral { .. } => ResolvedType::Date,
            Expression::TodLiteral { .. } => ResolvedType::Tod,
            Expression::DtLiteral { .. } => ResolvedType::Dt,

            Expression::Identifier { name, span } => {
                if let Some(sym) = self.symbols.lookup(name) {
                    sym.resolved_type.clone()
                } else {
                    self.error(span, format!("undeclared variable '{}'", name));
                    ResolvedType::Error
                }
            }

            Expression::BinaryOp {
                left,
                op,
                right,
                span,
            } => {
                let lt = self.check_expression(left);
                let rt = self.check_expression(right);

                if lt == ResolvedType::Error || rt == ResolvedType::Error {
                    return ResolvedType::Error;
                }

                self.check_binary_op(&lt, *op, &rt, span)
            }

            Expression::UnaryOp { op, operand, span } => {
                let ot = self.check_expression(operand);
                if ot == ResolvedType::Error {
                    return ResolvedType::Error;
                }

                match op {
                    UnaryOperator::Neg | UnaryOperator::Pos => {
                        if !ot.is_numeric() {
                            self.error(
                                span,
                                format!(
                                    "unary {} requires numeric operand, found {}",
                                    if *op == UnaryOperator::Neg { "-" } else { "+" },
                                    ot
                                ),
                            );
                            ResolvedType::Error
                        } else {
                            ot
                        }
                    }
                    UnaryOperator::Not => {
                        if ot != ResolvedType::Bool && !ot.is_integer() {
                            self.error(
                                span,
                                format!("NOT requires BOOL or integer operand, found {}", ot),
                            );
                            ResolvedType::Error
                        } else {
                            ot
                        }
                    }
                }
            }

            Expression::FunctionCall { name, args, span } => self.check_call(name, args, span),

            Expression::ArrayAccess {
                array,
                indices,
                span,
            } => {
                let arr_type = self.check_expression(array);
                for idx in indices {
                    let idx_type = self.check_expression(idx);
                    if idx_type != ResolvedType::Error && !idx_type.is_integer() {
                        self.error(
                            span,
                            format!("array index must be integer, found {}", idx_type),
                        );
                    }
                }
                if let ResolvedType::Array { element, ranges } = &arr_type {
                    if indices.len() != ranges.len() {
                        self.error(
                            span,
                            format!(
                                "expected {} array index(es), found {}",
                                ranges.len(),
                                indices.len()
                            ),
                        );
                    }
                    *element.clone()
                } else if arr_type != ResolvedType::Error {
                    self.error(
                        span,
                        format!("subscript applied to non-array type {}", arr_type),
                    );
                    ResolvedType::Error
                } else {
                    ResolvedType::Error
                }
            }

            Expression::MemberAccess {
                object,
                member,
                span,
            } => {
                let obj_type = self.check_expression(object);
                // Member access on FB instances — codegen will resolve
                // the actual field. For now, accept and return Error
                // (a more complete impl would look up the FB definition).
                if let ResolvedType::UserDefined { name } = &obj_type {
                    if let Some(pou) = self.symbols.lookup_pou(name) {
                        // Find the parameter
                        for (pname, ptype, _) in &pou.parameters {
                            if pname == member {
                                return ptype.clone();
                            }
                        }
                        self.error(span, format!("'{}' has no member '{}'", name, member));
                    }
                    ResolvedType::Error
                } else if obj_type != ResolvedType::Error {
                    self.error(
                        span,
                        format!("member access on non-structured type {}", obj_type),
                    );
                    ResolvedType::Error
                } else {
                    ResolvedType::Error
                }
            }
        }
    }

    // ── Binary operator type rules ──────────────────────────────

    fn check_binary_op(
        &mut self,
        lt: &ResolvedType,
        op: BinaryOperator,
        rt: &ResolvedType,
        span: &Span,
    ) -> ResolvedType {
        match op {
            // Arithmetic: both numeric, result is promoted type
            BinaryOperator::Add
            | BinaryOperator::Sub
            | BinaryOperator::Mul
            | BinaryOperator::Div
            | BinaryOperator::Power => {
                // Durations add and subtract like the integers they are
                // stored as: TIME ± TIME → TIME, TIME ± integer → TIME.
                if matches!(op, BinaryOperator::Add | BinaryOperator::Sub)
                    && (*lt == ResolvedType::Time || *rt == ResolvedType::Time)
                {
                    let other = if *lt == ResolvedType::Time { rt } else { lt };
                    if *other == ResolvedType::Time || other.is_integer() {
                        return ResolvedType::Time;
                    }
                }
                if !lt.is_numeric() || !rt.is_numeric() {
                    self.error(
                        span,
                        format!(
                            "operator {} requires numeric operands, found {} and {}",
                            op, lt, rt
                        ),
                    );
                    return ResolvedType::Error;
                }
                self.promote_numeric(lt, rt)
            }

            // MOD: integer only
            BinaryOperator::Mod => {
                if !lt.is_integer() || !rt.is_integer() {
                    self.error(
                        span,
                        format!("MOD requires integer operands, found {} and {}", lt, rt),
                    );
                    return ResolvedType::Error;
                }
                self.promote_numeric(lt, rt)
            }

            // Comparison: numeric, result is BOOL
            BinaryOperator::Eq
            | BinaryOperator::Neq
            | BinaryOperator::Lt
            | BinaryOperator::Le
            | BinaryOperator::Gt
            | BinaryOperator::Ge => {
                // Allow BOOL = BOOL comparison
                if *lt == ResolvedType::Bool && *rt == ResolvedType::Bool {
                    return ResolvedType::Bool;
                }
                // Durations compare against each other and against plain
                // integer millisecond counts.
                if (*lt == ResolvedType::Time && (*rt == ResolvedType::Time || rt.is_integer()))
                    || (*rt == ResolvedType::Time && lt.is_integer())
                {
                    return ResolvedType::Bool;
                }
                if !lt.is_numeric() || !rt.is_numeric() {
                    self.error(
                        span,
                        format!(
                            "comparison {} requires numeric operands, found {} and {}",
                            op, lt, rt
                        ),
                    );
                    return ResolvedType::Error;
                }
                ResolvedType::Bool
            }

            // Boolean: both BOOL or both integer (bitwise)
            BinaryOperator::And | BinaryOperator::Or | BinaryOperator::Xor => {
                if *lt == ResolvedType::Bool && *rt == ResolvedType::Bool {
                    return ResolvedType::Bool;
                }
                if lt.is_integer() && rt.is_integer() {
                    return self.promote_numeric(lt, rt);
                }
                self.error(
                    span,
                    format!(
                        "operator {} requires BOOL or integer operands, found {} and {}",
                        op, lt, rt
                    ),
                );
                ResolvedType::Error
            }
        }
    }

    /// IEC 61131-3 numeric promotion: if either operand is float,
    /// promote to the wider float. Otherwise, promote to the wider
    /// integer. Mixed signed/unsigned promotes to signed.
    fn promote_numeric(&self, a: &ResolvedType, b: &ResolvedType) -> ResolvedType {
        // Float wins over integer
        if a.is_float() || b.is_float() {
            let bits_a = match a {
                ResolvedType::Float { bits } => *bits,
                _ => 0,
            };
            let bits_b = match b {
                ResolvedType::Float { bits } => *bits,
                _ => 0,
            };
            return ResolvedType::Float {
                bits: bits_a.max(bits_b).max(32),
            };
        }

        // Both integers — take the wider width
        let bits_a = a.int_bits().unwrap_or(32);
        let bits_b = b.int_bits().unwrap_or(32);
        let max_bits = bits_a.max(bits_b);

        // Signed wins over unsigned in mixed contexts
        if a.is_signed() || b.is_signed() {
            ResolvedType::SignedInt { bits: max_bits }
        } else {
            // Check if either is a bit string
            match (a, b) {
                (ResolvedType::BitString { .. }, _) | (_, ResolvedType::BitString { .. }) => {
                    ResolvedType::BitString { bits: max_bits }
                }
                _ => ResolvedType::UnsignedInt { bits: max_bits },
            }
        }
    }

    // ── Assignment compatibility ─────────────────────────────────

    fn check_assignable(&mut self, target: &ResolvedType, value: &ResolvedType, span: &Span) {
        if target == value {
            return; // exact match
        }

        // Allow numeric implicit conversion (widening)
        if target.is_numeric() && value.is_numeric() {
            // Integer → float is ok, narrow → wide is ok
            // We issue a warning for potential precision loss
            if value.is_float() && target.is_integer() {
                self.warning(
                    span,
                    format!(
                        "implicit conversion from {} to {} may lose precision",
                        value, target
                    ),
                );
            }
            return; // numeric assignment allowed
        }

        // Allow BOOL to BOOL (already caught by exact match)
        // Disallow everything else
        self.error(span, format!("cannot assign {} to {}", value, target));
    }

    // ── Constraint helpers ──────────────────────────────────────

    fn require_bool(&mut self, ty: &ResolvedType, span: &Span, context: &str) {
        if *ty != ResolvedType::Bool && *ty != ResolvedType::Error {
            self.error(span, format!("{} must be BOOL, found {}", context, ty));
        }
    }

    fn require_numeric(&mut self, ty: &ResolvedType, span: &Span, context: &str) {
        if !ty.is_numeric() && *ty != ResolvedType::Error {
            self.error(span, format!("{} must be numeric, found {}", context, ty));
        }
    }

    // ── Call checking ───────────────────────────────────────────

    fn check_call(&mut self, name: &str, args: &[CallArg], span: &Span) -> ResolvedType {
        // The scan-clock intrinsic. It has no ST definition — codegen
        // lowers it to a load of the runtime's millisecond counter.
        if name.eq_ignore_ascii_case(crate::stdlib::TIME_MS_INTRINSIC) {
            if !args.is_empty() {
                self.error(
                    span,
                    format!("{}() takes no arguments", crate::stdlib::TIME_MS_INTRINSIC),
                );
            }
            return ResolvedType::Time;
        }

        // `inst(IN := x, Q => y);` — invoking a function block instance.
        // The callee name is a *variable*, so this check comes before the
        // POU lookup below.
        if let Some(sym) = self.symbols.lookup(name).cloned() {
            if let ResolvedType::UserDefined { name: fb_name } = &sym.resolved_type {
                match self.symbols.lookup_pou(fb_name).cloned() {
                    Some(pou) if pou.kind == PouKind::FunctionBlock => {
                        self.check_fb_invocation(name, fb_name, &pou, args, span);
                    }
                    _ => {
                        self.error(
                            span,
                            format!(
                                "'{}' is declared as '{}', which is not a known function block",
                                name, fb_name
                            ),
                        );
                    }
                }
                return ResolvedType::Bool;
            }
        }

        if let Some(pou) = self.symbols.lookup_pou(name).cloned() {
            self.check_call_arg_expressions(args);
            // Return the function's return type, or BOOL as default for FB/PROGRAM
            pou.return_type.unwrap_or(ResolvedType::Bool)
        } else {
            // Could be a built-in function — accept with a warning for now
            self.check_call_arg_expressions(args);
            self.warning(
                span,
                format!("call to unknown function/FB '{}' — assuming valid", name),
            );
            ResolvedType::SignedInt { bits: 32 } // default return type
        }
    }

    /// Type-checks the arguments of a function block instance call
    /// against the block's declared VAR_INPUT / VAR_OUTPUT parameters.
    ///
    /// Inputs arrive as positional or `name := value` arguments; outputs
    /// are extracted with `name => target`. Reading an output through
    /// `inst.Q` afterwards is handled by member access instead, and is
    /// the more common style.
    fn check_fb_invocation(
        &mut self,
        instance: &str,
        fb_name: &str,
        pou: &PouInfo,
        args: &[CallArg],
        span: &Span,
    ) {
        let inputs: Vec<&(String, ResolvedType, VarQualifier)> = pou
            .parameters
            .iter()
            .filter(|(_, _, q)| matches!(q, VarQualifier::VarInput | VarQualifier::VarInOut))
            .collect();

        let mut positional = 0usize;
        for arg in args {
            match arg {
                CallArg::Positional(expr) => {
                    let value_type = self.check_expression(expr);
                    match inputs.get(positional) {
                        Some((_, param_type, _)) => {
                            if value_type != ResolvedType::Error {
                                self.check_assignable(param_type, &value_type, span);
                            }
                        }
                        None => self.error(
                            span,
                            format!(
                                "'{}' ({}) takes {} input(s), but more were given",
                                instance,
                                fb_name,
                                inputs.len()
                            ),
                        ),
                    }
                    positional += 1;
                }

                CallArg::Named { name, value } => {
                    let value_type = self.check_expression(value);
                    match Self::find_parameter(pou, name) {
                        Some((_, param_type, VarQualifier::VarInput))
                        | Some((_, param_type, VarQualifier::VarInOut)) => {
                            if value_type != ResolvedType::Error {
                                self.check_assignable(param_type, &value_type, span);
                            }
                        }
                        Some((_, _, _)) => self.error(
                            span,
                            format!(
                                "'{}' is an output of {} — read it as {}.{} or bind it with '=>'",
                                name, fb_name, instance, name
                            ),
                        ),
                        None => self.error(
                            span,
                            format!("{} has no input named '{}'", fb_name, name),
                        ),
                    }
                }

                CallArg::Output { name, target } => {
                    let target_type = self.check_expression(target);
                    match Self::find_parameter(pou, name) {
                        Some((_, param_type, VarQualifier::VarOutput))
                        | Some((_, param_type, VarQualifier::VarInOut)) => {
                            if target_type != ResolvedType::Error {
                                self.check_assignable(&target_type, param_type, span);
                            }
                        }
                        Some((_, _, _)) => self.error(
                            span,
                            format!("'{}' is an input of {} — assign it with ':='", name, fb_name),
                        ),
                        None => self.error(
                            span,
                            format!("{} has no output named '{}'", fb_name, name),
                        ),
                    }
                }
            }
        }
    }

    fn find_parameter<'p>(
        pou: &'p PouInfo,
        name: &str,
    ) -> Option<&'p (String, ResolvedType, VarQualifier)> {
        pou.parameters
            .iter()
            .find(|(param, _, _)| param.eq_ignore_ascii_case(name))
    }

    /// Type-checks argument expressions without binding them to
    /// parameters — used for calls whose callee has no known signature.
    fn check_call_arg_expressions(&mut self, args: &[CallArg]) {
        for arg in args {
            match arg {
                CallArg::Positional(expr) | CallArg::Named { value: expr, .. } => {
                    self.check_expression(expr);
                }
                CallArg::Output { target, .. } => {
                    self.check_expression(target);
                }
            }
        }
    }
}

// ─── Convenience function ───────────────────────────────────────

/// Analyses a compilation unit and returns the program context.
///
/// This is the main entry point for semantic analysis.
///

pub fn analyze(ast: CompilationUnit) -> ProgramContext {
    SemanticAnalyzer::new().analyze(ast)
}

// ─── Unit Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn analyze_src(src: &str) -> ProgramContext {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer);
        let ast = parser
            .parse()
            .unwrap_or_else(|e| panic!("Parse error: {}", e));
        analyze(ast)
    }

    fn assert_no_errors(src: &str) -> ProgramContext {
        let ctx = analyze_src(src);
        if ctx.has_errors() {
            for d in &ctx.diagnostics {
                eprintln!("  {}", d);
            }
            panic!("Expected no errors, found {}", ctx.error_count());
        }
        ctx
    }

    fn assert_has_errors(src: &str) -> ProgramContext {
        let ctx = analyze_src(src);
        assert!(ctx.has_errors(), "Expected errors, found none");
        ctx
    }

    // ── Type resolution ─────────────────────────────────────────

    #[test]
    fn test_resolve_elementary_types() {
        let mut sa = SemanticAnalyzer::new();
        assert_eq!(
            sa.resolve_type(&TypeSpec::Elementary(ElementaryType::Bool)),
            ResolvedType::Bool
        );
        assert_eq!(
            sa.resolve_type(&TypeSpec::Elementary(ElementaryType::Int)),
            ResolvedType::SignedInt { bits: 16 }
        );
        assert_eq!(
            sa.resolve_type(&TypeSpec::Elementary(ElementaryType::Dint)),
            ResolvedType::SignedInt { bits: 32 }
        );
        assert_eq!(
            sa.resolve_type(&TypeSpec::Elementary(ElementaryType::Real)),
            ResolvedType::Float { bits: 32 }
        );
        assert_eq!(
            sa.resolve_type(&TypeSpec::Elementary(ElementaryType::Lreal)),
            ResolvedType::Float { bits: 64 }
        );
        assert_eq!(
            sa.resolve_type(&TypeSpec::Elementary(ElementaryType::Dword)),
            ResolvedType::BitString { bits: 32 }
        );
    }

    #[test]
    fn test_resolve_array_type() {
        let mut sa = SemanticAnalyzer::new();
        let ts = TypeSpec::Array {
            ranges: vec![ArrayRange { low: 0, high: 9 }],
            element_type: Box::new(TypeSpec::Elementary(ElementaryType::Dint)),
        };
        let resolved = sa.resolve_type(&ts);
        if let ResolvedType::Array { element, ranges } = resolved {
            assert_eq!(*element, ResolvedType::SignedInt { bits: 32 });
            assert_eq!(ranges.len(), 1);
        } else {
            panic!("Expected Array");
        }
    }

    // ── Variable declaration ────────────────────────────────────

    #[test]
    fn test_valid_declarations() {
        assert_no_errors(
            "PROGRAM P VAR x : INT := 0; y : REAL := 3.14; z : BOOL := TRUE; END_VAR END_PROGRAM",
        );
    }

    #[test]
    fn test_duplicate_variable() {
        let ctx = assert_has_errors("PROGRAM P VAR x : INT; x : REAL; END_VAR END_PROGRAM");
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("already declared"))
        );
    }

    // ── Undeclared variables ────────────────────────────────────

    #[test]
    fn test_undeclared_variable() {
        let ctx = assert_has_errors("PROGRAM P VAR x : INT; END_VAR y := 42; END_PROGRAM");
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("undeclared variable 'y'"))
        );
    }

    #[test]
    fn test_valid_variable_usage() {
        assert_no_errors("PROGRAM P VAR x : INT := 0; END_VAR x := x + 1; END_PROGRAM");
    }

    // ── Constant enforcement ────────────────────────────────────

    #[test]
    fn test_constant_assignment() {
        let ctx = assert_has_errors(
            "PROGRAM P VAR CONSTANT MAX : INT := 100; END_VAR MAX := 200; END_PROGRAM",
        );
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("cannot assign to CONSTANT"))
        );
    }

    // ── Type checking: assignments ──────────────────────────────

    #[test]
    fn test_numeric_assignment_ok() {
        // INT := DINT — numeric widening is allowed
        assert_no_errors("PROGRAM P VAR x : REAL; END_VAR x := 42; END_PROGRAM");
    }

    #[test]
    fn test_bool_to_int_assignment() {
        let ctx = assert_has_errors("PROGRAM P VAR x : INT; END_VAR x := TRUE; END_PROGRAM");
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("cannot assign"))
        );
    }

    // ── Type checking: operators ────────────────────────────────

    #[test]
    fn test_arithmetic_requires_numeric() {
        let ctx =
            assert_has_errors("PROGRAM P VAR x : INT; b : BOOL; END_VAR x := x + b; END_PROGRAM");
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("requires numeric operands"))
        );
    }

    #[test]
    fn test_mod_requires_integer() {
        let ctx = assert_has_errors("PROGRAM P VAR x : REAL; END_VAR x := x MOD 2.0; END_PROGRAM");
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("MOD requires integer"))
        );
    }

    #[test]
    fn test_boolean_operators_ok() {
        assert_no_errors(
            "PROGRAM P VAR a : BOOL; b : BOOL; c : BOOL; END_VAR \
             c := a AND b OR NOT c; END_PROGRAM",
        );
    }

    #[test]
    fn test_comparison_produces_bool() {
        assert_no_errors(
            "PROGRAM P VAR x : INT; flag : BOOL; END_VAR \
             flag := x > 0; END_PROGRAM",
        );
    }

    // ── Control flow validation ─────────────────────────────────

    #[test]
    fn test_if_requires_bool() {
        let ctx = assert_has_errors(
            "PROGRAM P VAR x : INT; END_VAR \
             IF x THEN x := 0; END_IF; END_PROGRAM",
        );
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("must be BOOL"))
        );
    }

    #[test]
    fn test_while_requires_bool() {
        let ctx = assert_has_errors(
            "PROGRAM P VAR x : INT; END_VAR \
             WHILE x DO x := x - 1; END_WHILE; END_PROGRAM",
        );
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("must be BOOL"))
        );
    }

    #[test]
    fn test_for_requires_integer_var() {
        let ctx = assert_has_errors(
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
        let ctx = assert_has_errors("PROGRAM P VAR x : INT; END_VAR EXIT; END_PROGRAM");
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("EXIT is only valid inside a loop"))
        );
    }

    #[test]
    fn test_exit_inside_loop_ok() {
        assert_no_errors(
            "PROGRAM P VAR x : INT := 0; END_VAR \
             WHILE x < 10 DO x := x + 1; EXIT; END_WHILE; END_PROGRAM",
        );
    }

    // ── Case statement ──────────────────────────────────────────

    #[test]
    fn test_case_requires_integer() {
        let ctx = assert_has_errors(
            "PROGRAM P VAR r : REAL; END_VAR \
             CASE r OF 0: r := 1.0; END_CASE; END_PROGRAM",
        );
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("CASE selector must be integer"))
        );
    }

    // ── Function declarations ───────────────────────────────────

    #[test]
    fn test_function_return_assignment() {
        assert_no_errors(
            "FUNCTION Add : INT \
             VAR_INPUT a : INT; b : INT; END_VAR \
             Add := a + b; \
             END_FUNCTION",
        );
    }

    // ── Array access ────────────────────────────────────────────

    #[test]
    fn test_array_access_valid() {
        assert_no_errors(
            "PROGRAM P VAR a : ARRAY[0..9] OF INT; i : INT; END_VAR \
             a[i] := a[i] + 1; END_PROGRAM",
        );
    }

    #[test]
    fn test_array_access_non_integer_index() {
        let ctx = assert_has_errors(
            "PROGRAM P VAR a : ARRAY[0..9] OF INT; r : REAL; END_VAR \
             a[r] := 0; END_PROGRAM",
        );
        assert!(
            ctx.diagnostics
                .iter()
                .any(|d| d.message.contains("array index must be integer"))
        );
    }

    // ── Realistic program ───────────────────────────────────────

    #[test]
    fn test_conveyor_control() {
        assert_no_errors(
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

    // ── Numeric promotion ───────────────────────────────────────

    #[test]
    fn test_promotion_int_real() {
        let sa = SemanticAnalyzer::new();
        let result = sa.promote_numeric(
            &ResolvedType::SignedInt { bits: 16 },
            &ResolvedType::Float { bits: 32 },
        );
        assert_eq!(result, ResolvedType::Float { bits: 32 });
    }

    #[test]
    fn test_promotion_signed_unsigned() {
        let sa = SemanticAnalyzer::new();
        let result = sa.promote_numeric(
            &ResolvedType::SignedInt { bits: 16 },
            &ResolvedType::UnsignedInt { bits: 32 },
        );
        // Signed wins, width is max
        assert_eq!(result, ResolvedType::SignedInt { bits: 32 });
    }
}
