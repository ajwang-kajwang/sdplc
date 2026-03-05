//! Abstract Syntax Tree (AST) definitions for IEC 61131-3 Structured Text.
//!
//! These types represent the parsed structure of an ST program. The parser
//! produces an AST, which semantic analysis validates, and LLVM IR generation
//! then walks to emit code.
//!
//! The design follows the IEC 61131-3:2025 grammar closely, with practical
//! simplifications for the Structured Text subset.

use std::fmt;

// ─── Source Location ────────────────────────────────────────────

/// A span in the source code for error reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

// ─── Top-Level Declarations ─────────────────────────────────────

/// A complete compilation unit — one or more Program Organisation Units.
#[derive(Debug, Clone)]
pub struct CompilationUnit {
    pub units: Vec<Pou>,
}

/// A Program Organisation Unit (POU): PROGRAM, FUNCTION, or FUNCTION_BLOCK.
#[derive(Debug, Clone)]
pub enum Pou {
    Program(ProgramDecl),
    Function(FunctionDecl),
    FunctionBlock(FunctionBlockDecl),
}

/// `PROGRAM name ... END_PROGRAM`
#[derive(Debug, Clone)]
pub struct ProgramDecl {
    pub name: String,
    pub var_blocks: Vec<VarBlock>,
    pub body: Vec<Statement>,
    pub span: Span,
}

/// `FUNCTION name : return_type ... END_FUNCTION`
#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub name: String,
    pub return_type: TypeSpec,
    pub var_blocks: Vec<VarBlock>,
    pub body: Vec<Statement>,
    pub span: Span,
}

/// `FUNCTION_BLOCK name ... END_FUNCTION_BLOCK`
#[derive(Debug, Clone)]
pub struct FunctionBlockDecl {
    pub name: String,
    pub var_blocks: Vec<VarBlock>,
    pub body: Vec<Statement>,
    pub span: Span,
}

// ─── Variable Declarations ──────────────────────────────────────

/// The qualifier on a VAR block: VAR, VAR_INPUT, VAR_OUTPUT, etc.
#[derive(Debug, Clone, PartialEq)]
pub enum VarQualifier {
    Var,
    VarInput,
    VarOutput,
    VarInOut,
    VarGlobal,
    VarExternal,
    VarTemp,
}

/// A variable declaration block.
///
/// ```text
/// VAR RETAIN CONSTANT
///     x : INT := 0;
///     y : REAL;
/// END_VAR
/// ```
#[derive(Debug, Clone)]
pub struct VarBlock {
    pub qualifier: VarQualifier,
    pub retain: bool,
    pub constant: bool,
    pub declarations: Vec<VarDecl>,
    pub span: Span,
}

/// A single variable declaration within a VAR block.
#[derive(Debug, Clone)]
pub struct VarDecl {
    pub name: String,
    pub type_spec: TypeSpec,
    pub initial_value: Option<Expression>,
    pub span: Span,
}

// ─── Type Specifications ────────────────────────────────────────

/// A type specifier — used in variable declarations and function return types.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeSpec {
    /// An elementary type: BOOL, INT, DINT, REAL, etc.
    Elementary(ElementaryType),
    /// `ARRAY[lo..hi] OF element_type`
    Array {
        ranges: Vec<ArrayRange>,
        element_type: Box<TypeSpec>,
    },
    /// `STRING` or `STRING[length]`
    StringType { max_len: Option<u32> },
    /// `WSTRING` or `WSTRING[length]`
    WStringType { max_len: Option<u32> },
    /// A user-defined type name (resolved during semantic analysis).
    UserDefined(String),
}

/// Elementary (built-in) data types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ElementaryType {
    Bool,
    Sint,
    Int,
    Dint,
    Lint,
    Usint,
    Uint,
    Udint,
    Ulint,
    Real,
    Lreal,
    Byte,
    Word,
    Dword,
    Lword,
    Time,
    Date,
    Tod,
    Dt,
}

/// A single dimension range in an ARRAY declaration: `lo..hi`
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayRange {
    pub low: i64,
    pub high: i64,
}

// ─── Statements ─────────────────────────────────────────────────

/// A statement in the body of a POU.
#[derive(Debug, Clone)]
pub enum Statement {
    /// `target := expression;`
    Assignment {
        target: Expression,
        value: Expression,
        span: Span,
    },
    /// `IF ... THEN ... {ELSIF ... THEN ...} [ELSE ...] END_IF`
    If {
        condition: Expression,
        then_body: Vec<Statement>,
        elsif_branches: Vec<(Expression, Vec<Statement>)>,
        else_body: Option<Vec<Statement>>,
        span: Span,
    },
    /// `FOR var := from TO to [BY step] DO ... END_FOR`
    For {
        variable: String,
        from: Expression,
        to: Expression,
        by: Option<Expression>,
        body: Vec<Statement>,
        span: Span,
    },
    /// `WHILE condition DO ... END_WHILE`
    While {
        condition: Expression,
        body: Vec<Statement>,
        span: Span,
    },
    /// `REPEAT ... UNTIL condition END_REPEAT`
    Repeat {
        body: Vec<Statement>,
        condition: Expression,
        span: Span,
    },
    /// `CASE selector OF ... END_CASE`
    Case {
        selector: Expression,
        branches: Vec<CaseBranch>,
        else_body: Option<Vec<Statement>>,
        span: Span,
    },
    /// `EXIT;` — break out of innermost loop.
    Exit { span: Span },
    /// `RETURN;` — early return from function/FB.
    Return { span: Span },
    /// A bare function or FB call as a statement: `MyFB(x := 1);`
    CallStatement {
        name: String,
        args: Vec<CallArg>,
        span: Span,
    },
    /// An empty statement (bare `;`).
    Empty,
}

/// A single branch in a CASE statement.
#[derive(Debug, Clone)]
pub struct CaseBranch {
    pub labels: Vec<CaseLabel>,
    pub body: Vec<Statement>,
}

/// A label in a CASE branch — either a single value or a range.
#[derive(Debug, Clone)]
pub enum CaseLabel {
    /// A single value: `0`, `42`, `-1`
    Value(Expression),
    /// A range: `1..5`
    Range(Expression, Expression),
}

/// An argument in a function/FB call.
#[derive(Debug, Clone)]
pub enum CallArg {
    /// Positional argument: just an expression.
    Positional(Expression),
    /// Named argument: `param_name := expression`.
    Named { name: String, value: Expression },
    /// Output argument: `param_name => variable`.
    Output { name: String, target: Expression },
}

// ─── Expressions ────────────────────────────────────────────────

/// An expression — the core recursive structure of ST computations.
#[derive(Debug, Clone)]
pub enum Expression {
    /// An integer literal: `42`, `16#FF`, `1_000`
    IntLiteral { value: i64, span: Span },
    /// A real literal: `3.14`, `1.0e-3`
    RealLiteral { value: f64, span: Span },
    /// A boolean literal: `TRUE` or `FALSE`
    BoolLiteral { value: bool, span: Span },
    /// A string literal: `'hello'`
    StringLiteral { value: String, span: Span },
    /// A wide string literal: `"hello"`
    WStringLiteral { value: String, span: Span },
    /// A time literal: `T#5s`
    TimeLiteral { text: String, span: Span },
    /// A date literal: `D#2025-12-01`
    DateLiteral { text: String, span: Span },
    /// A time-of-day literal: `TOD#14:30:00`
    TodLiteral { text: String, span: Span },
    /// A date-and-time literal: `DT#2025-12-01-14:30:00`
    DtLiteral { text: String, span: Span },
    /// A variable reference: `speed`, `count`
    Identifier { name: String, span: Span },
    /// A binary operation: `a + b`, `x AND y`, `i <= 10`
    BinaryOp {
        left: Box<Expression>,
        op: BinaryOperator,
        right: Box<Expression>,
        span: Span,
    },
    /// A unary operation: `-x`, `NOT flag`
    UnaryOp {
        op: UnaryOperator,
        operand: Box<Expression>,
        span: Span,
    },
    /// A function call: `ABS(x)`, `MAX(a, b)`
    FunctionCall {
        name: String,
        args: Vec<CallArg>,
        span: Span,
    },
    /// Array subscript: `arr[i]`, `matrix[r, c]`
    ArrayAccess {
        array: Box<Expression>,
        indices: Vec<Expression>,
        span: Span,
    },
    /// Member access: `fb_instance.output`
    MemberAccess {
        object: Box<Expression>,
        member: String,
        span: Span,
    },
}

/// Binary operators, ordered roughly by precedence group for reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOperator {
    // Arithmetic
    Add,        // +
    Sub,        // -
    Mul,        // *
    Div,        // /
    Mod,        // MOD
    Power,      // **

    // Comparison
    Eq,         // =
    Neq,        // <>
    Lt,         // <
    Le,         // <=
    Gt,         // >
    Ge,         // >=

    // Boolean / Bitwise
    And,        // AND, &
    Or,         // OR
    Xor,        // XOR
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOperator {
    Neg,        // -
    Pos,        // + (no-op, but parsed)
    Not,        // NOT
}

// ─── Display implementations for debugging ──────────────────────

impl fmt::Display for ElementaryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool  => write!(f, "BOOL"),
            Self::Sint  => write!(f, "SINT"),
            Self::Int   => write!(f, "INT"),
            Self::Dint  => write!(f, "DINT"),
            Self::Lint  => write!(f, "LINT"),
            Self::Usint => write!(f, "USINT"),
            Self::Uint  => write!(f, "UINT"),
            Self::Udint => write!(f, "UDINT"),
            Self::Ulint => write!(f, "ULINT"),
            Self::Real  => write!(f, "REAL"),
            Self::Lreal => write!(f, "LREAL"),
            Self::Byte  => write!(f, "BYTE"),
            Self::Word  => write!(f, "WORD"),
            Self::Dword => write!(f, "DWORD"),
            Self::Lword => write!(f, "LWORD"),
            Self::Time  => write!(f, "TIME"),
            Self::Date  => write!(f, "DATE"),
            Self::Tod   => write!(f, "TOD"),
            Self::Dt    => write!(f, "DT"),
        }
    }
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add   => write!(f, "+"),
            Self::Sub   => write!(f, "-"),
            Self::Mul   => write!(f, "*"),
            Self::Div   => write!(f, "/"),
            Self::Mod   => write!(f, "MOD"),
            Self::Power => write!(f, "**"),
            Self::Eq    => write!(f, "="),
            Self::Neq   => write!(f, "<>"),
            Self::Lt    => write!(f, "<"),
            Self::Le    => write!(f, "<="),
            Self::Gt    => write!(f, ">"),
            Self::Ge    => write!(f, ">="),
            Self::And   => write!(f, "AND"),
            Self::Or    => write!(f, "OR"),
            Self::Xor   => write!(f, "XOR"),
        }
    }
}