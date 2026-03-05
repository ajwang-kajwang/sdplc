//! # SD-PLC
//!
//! An IEC 61131-3 Structured Text compiler targeting LLVM IR
//! for cross-platform industrial control.
//!
//! This crate implements the **lexer**, **parser**, and **semantic
//! analysis** stages of the compilation pipeline, converting raw
//! Structured Text source code into a validated, type-resolved
//! [`ProgramContext`](semantic::ProgramContext) ready for LLVM IR
//! generation via `inkwell`.
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────┐    ┌────────┐    ┌────────┐    ┌──────────┐    ┌─────────┐
//! │ ST Source   │───▶│ Lexer  │───▶│ Parser │───▶│ Semantic │───▶│ LLVM IR │
//! │ (PLCopen)   │    │   ✅   │    │   ✅   │    │    ✅    │    │  (TODO) │
//! └────────────┘    └────────┘    └────────┘    └──────────┘    └─────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```
//! use sdplc::lexer::Lexer;
//! use sdplc::parser::Parser;
//! use sdplc::semantic::analyze;
//!
//! let source = "PROGRAM Main VAR x : INT := 0; END_VAR x := x + 1; END_PROGRAM";
//! let lexer = Lexer::new(source);
//! let mut parser = Parser::new(lexer);
//! let ast = parser.parse().expect("parse error");
//! let ctx = analyze(ast);
//!
//! assert!(!ctx.has_errors(), "semantic errors found");
//! ```

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod semantic;