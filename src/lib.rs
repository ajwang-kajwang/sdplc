//! # SD-PLC
//!
//! An IEC 61131-3 Structured Text compiler targeting LLVM IR
//! for cross-platform industrial control.
//!
//! This crate currently implements the **lexer** stage of the
//! compilation pipeline, converting raw Structured Text source
//! code into a token stream suitable for parsing.
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────┐    ┌────────┐    ┌──────────┐    ┌─────────┐
//! │ ST Source   │───▶│ Lexer  │───▶│  Parser  │───▶│ LLVM IR │
//! │ (PLCopen)   │    │        │    │  (TODO)  │    │  (TODO) │
//! └────────────┘    └────────┘    └──────────┘    └─────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```
//! use sdplc::lexer::{Lexer, TokenType};
//!
//! let mut lexer = Lexer::new("PROGRAM Main END_PROGRAM");
//! let tokens = lexer.tokenize();
//!
//! assert_eq!(tokens[0].kind, TokenType::Program);
//! assert_eq!(tokens[0].text, "PROGRAM");
//! ```

pub mod lexer;