//! # SD-PLC
//!
//! An IEC 61131-3 Structured Text compiler targeting LLVM IR
//! for cross-platform industrial control.
//!
//! This crate implements the complete compiler frontend and LLVM
//! code generation pipeline: **lexer** → **parser** → **semantic
//! analysis** → **LLVM IR generation** via `inkwell`.
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────┐    ┌────────┐    ┌────────┐    ┌──────────┐    ┌─────────┐
//! │ ST Source   │───▶│ Lexer  │───▶│ Parser │───▶│ Semantic │───▶│ Codegen │
//! │ (PLCopen)   │    │   ✅   │    │   ✅   │    │    ✅    │    │   ✅    │
//! └────────────┘    └────────┘    └────────┘    └──────────┘    └─────────┘
//!                                                                     │
//!                                                    ┌────────────────┤
//!                                                    ▼                ▼
//!                                               Native code     WebAssembly
//!                                               (x86, ARM)      (portable)
//! ```
//!
//! ## Quick Start
//!
//! ```ignore
//! use sdplc::lexer::Lexer;
//! use sdplc::parser::Parser;
//! use sdplc::semantic;
//! use sdplc::codegen::CodeGenerator;
//! use inkwell::context::Context;
//!
//! let source = "PROGRAM P VAR x : INT := 0; END_VAR x := x + 1; END_PROGRAM";
//! let lexer = Lexer::new(source);
//! let mut parser = Parser::new(lexer);
//! let ast = parser.parse().expect("parse error");
//!
//! let ctx = semantic::analyze(ast.clone());
//! assert!(!ctx.has_errors());
//!
//! let llvm_ctx = Context::create();
//! let mut codegen = CodeGenerator::new(&llvm_ctx, "my_plc");
//! codegen.compile(&ast).expect("codegen error");
//! codegen.write_ir("output.ll").unwrap();
//! ```

pub mod ast;
pub mod codegen;
pub mod lexer;
pub mod parser;
pub mod semantic;