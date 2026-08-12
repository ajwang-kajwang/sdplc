//! The IEC 61131-3 standard function block library.
//!
//! The blocks themselves are written in Structured Text
//! ([`STANDARD_FB_SOURCE`], `src/stdlib/standard_fb.st`) and go through
//! the same lexer → parser → semantic → codegen pipeline as user code.
//! Nothing in this module special-cases an individual block; adding one
//! means adding a `FUNCTION_BLOCK` to that `.st` file and nothing else.
//!
//! ## Injection
//!
//! [`inject`] prepends the standard blocks a compilation unit actually
//! needs — determined by scanning declared variable types transitively —
//! so a program with no timers gets no timer IR. Both
//! [`semantic::analyze`](crate::semantic::analyze) and the two
//! [`CodeGenerator`](crate::codegen::CodeGenerator) entry points call it
//! internally, so callers never have to remember to.
//!
//! ## TIME representation
//!
//! `TIME` is a signed 64-bit count of **milliseconds**. The timers read
//! the current time through the `TIME_MS()` intrinsic, which lowers to a
//! load of the `@__sdplc_now_ms` global that the runtime advances once
//! per scan cycle — see [`codegen`](crate::codegen) and Developer Guide §13.

use std::collections::HashSet;

use crate::ast::{CompilationUnit, Pou, TypeSpec, VarBlock};
use crate::lexer::Lexer;
use crate::parser::Parser;

/// Structured Text source for every standard function block.
pub const STANDARD_FB_SOURCE: &str = include_str!("stdlib/standard_fb.st");

/// The intrinsic function that reads the runtime scan clock, in
/// milliseconds. Recognised by semantic analysis and lowered by codegen;
/// it has no ST definition.
pub const TIME_MS_INTRINSIC: &str = "TIME_MS";

/// Parses [`STANDARD_FB_SOURCE`] into POUs.
///
/// # Panics
///
/// Panics if the bundled library fails to parse — that is a build-time
/// defect in `standard_fb.st`, not a user error. The unit tests in this
/// module keep it from reaching a release.
pub fn standard_pous() -> Vec<Pou> {
    let lexer = Lexer::new(STANDARD_FB_SOURCE);
    let mut parser = Parser::new(lexer);
    match parser.parse() {
        Ok(unit) => unit.units,
        Err(e) => panic!("SD-PLC standard function block library failed to parse: {e}"),
    }
}

/// Returns the names of all standard function blocks.
pub fn standard_fb_names() -> Vec<String> {
    standard_pous()
        .iter()
        .map(|pou| match pou {
            Pou::Program(p) => p.name.clone(),
            Pou::Function(f) => f.name.clone(),
            Pou::FunctionBlock(fb) => fb.name.clone(),
        })
        .collect()
}

/// Returns a copy of `unit` with the standard function blocks it needs
/// prepended to its POU list.
///
/// A block is needed when the program declares an instance of it, or
/// when another needed block does (blocks may compose). Blocks the user
/// has defined themselves under the same name win — the bundled version
/// is skipped rather than producing a duplicate definition.
pub fn inject(unit: &CompilationUnit) -> CompilationUnit {
    let library = standard_pous();

    // Names the user already defines shadow the bundled library.
    let user_defined: HashSet<String> = unit
        .units
        .iter()
        .filter_map(|pou| match pou {
            Pou::FunctionBlock(fb) => Some(fb.name.clone()),
            Pou::Function(f) => Some(f.name.clone()),
            Pou::Program(_) => None,
        })
        .collect();

    // Seed with the types the user program mentions, then close over the
    // library's own dependencies so a block built from other blocks works.
    let mut needed: HashSet<String> = HashSet::new();
    let mut frontier: Vec<String> = referenced_type_names(&unit.units)
        .into_iter()
        .filter(|name| !user_defined.contains(name))
        .collect();

    while let Some(name) = frontier.pop() {
        if !needed.insert(name.clone()) {
            continue;
        }
        if let Some(pou) = find_pou(&library, &name) {
            for dep in referenced_type_names(std::slice::from_ref(pou)) {
                if !needed.contains(&dep) && !user_defined.contains(&dep) {
                    frontier.push(dep);
                }
            }
        }
    }

    let mut units: Vec<Pou> = library
        .into_iter()
        .filter(|pou| match pou {
            Pou::FunctionBlock(fb) => needed.contains(&fb.name),
            Pou::Function(f) => needed.contains(&f.name),
            Pou::Program(_) => false,
        })
        .collect();
    units.extend(unit.units.iter().cloned());

    CompilationUnit { units }
}

fn find_pou<'a>(pous: &'a [Pou], name: &str) -> Option<&'a Pou> {
    pous.iter().find(|pou| match pou {
        Pou::Program(p) => p.name == name,
        Pou::Function(f) => f.name == name,
        Pou::FunctionBlock(fb) => fb.name == name,
    })
}

/// Collects every `UserDefined` type name declared anywhere in `pous` —
/// these are the candidate function block instantiations.
fn referenced_type_names(pous: &[Pou]) -> Vec<String> {
    let mut names = Vec::new();
    for pou in pous {
        let blocks: &[VarBlock] = match pou {
            Pou::Program(p) => &p.var_blocks,
            Pou::Function(f) => &f.var_blocks,
            Pou::FunctionBlock(fb) => &fb.var_blocks,
        };
        for block in blocks {
            for decl in &block.declarations {
                collect_type_names(&decl.type_spec, &mut names);
            }
        }
    }
    names
}

fn collect_type_names(ts: &TypeSpec, out: &mut Vec<String>) {
    match ts {
        TypeSpec::UserDefined(name) => out.push(name.clone()),
        TypeSpec::Array { element_type, .. } => collect_type_names(element_type, out),
        _ => {}
    }
}

// ─── Time literals ──────────────────────────────────────────────

/// Parses an IEC 61131-3 duration literal into milliseconds.
///
/// Accepts the `T#`, `TIME#`, `t#` prefixes, an optional leading sign,
/// underscore digit separators, and the `d`, `h`, `m`, `s`, `ms` unit
/// suffixes in any combination: `T#5s`, `T#1h30m`, `T#-2m10s500ms`.
/// A bare number with no unit is read as milliseconds.
///
/// Returns `None` if the text is not a well-formed duration.
pub fn parse_time_literal(text: &str) -> Option<i64> {
    let body = text
        .split_once('#')
        .map(|(_, rest)| rest)
        .unwrap_or(text)
        .replace('_', "");

    let (negative, digits) = match body.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, body.strip_prefix('+').unwrap_or(&body)),
    };
    if digits.is_empty() {
        return None;
    }

    let mut total_ms: f64 = 0.0;
    let mut number = String::new();
    let mut unit = String::new();
    let mut saw_component = false;

    // Walk the literal accumulating `<number><unit>` pairs. A unit is
    // only complete once a digit (or the end of input) follows it, so
    // the two-letter `ms` is not mistaken for `m` then `s`.
    for ch in digits.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            if !unit.is_empty() {
                total_ms += scale_component(&number, &unit)?;
                saw_component = true;
                number.clear();
                unit.clear();
            }
            number.push(ch);
        } else if ch.is_ascii_alphabetic() {
            if number.is_empty() {
                return None;
            }
            unit.push(ch.to_ascii_lowercase());
        } else {
            return None;
        }
    }

    if !number.is_empty() {
        if unit.is_empty() {
            // Unit-less trailing value: milliseconds.
            total_ms += number.parse::<f64>().ok()?;
        } else {
            total_ms += scale_component(&number, &unit)?;
        }
        saw_component = true;
    } else if !unit.is_empty() {
        return None;
    }

    if !saw_component {
        return None;
    }

    let ms = total_ms.round() as i64;
    Some(if negative { -ms } else { ms })
}

fn scale_component(number: &str, unit: &str) -> Option<f64> {
    let value: f64 = number.parse().ok()?;
    let scale = match unit {
        "d" => 86_400_000.0,
        "h" => 3_600_000.0,
        "m" => 60_000.0,
        "s" => 1_000.0,
        "ms" => 1.0,
        _ => return None,
    };
    Some(value * scale)
}

// ─── Unit Tests ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::CompilationUnit;

    fn parse(src: &str) -> CompilationUnit {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer);
        parser.parse().expect("test program should parse")
    }

    #[test]
    fn library_parses() {
        let names = standard_fb_names();
        for expected in [
            "TON", "TOF", "TP", "CTU", "CTD", "CTUD", "R_TRIG", "F_TRIG", "RS", "SR",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn injects_only_what_is_used() {
        let unit = parse("PROGRAM P VAR t : TON; END_VAR END_PROGRAM");
        let injected = inject(&unit);
        let names: Vec<String> = injected
            .units
            .iter()
            .filter_map(|pou| match pou {
                Pou::FunctionBlock(fb) => Some(fb.name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["TON".to_string()]);
    }

    #[test]
    fn injects_nothing_when_unused() {
        let unit = parse("PROGRAM P VAR x : INT; END_VAR END_PROGRAM");
        let injected = inject(&unit);
        assert_eq!(injected.units.len(), 1);
    }

    #[test]
    fn user_definition_shadows_library() {
        let unit = parse(
            "FUNCTION_BLOCK TON VAR_OUTPUT Q : BOOL; END_VAR END_FUNCTION_BLOCK \
             PROGRAM P VAR t : TON; END_VAR END_PROGRAM",
        );
        let injected = inject(&unit);
        let ton_count = injected
            .units
            .iter()
            .filter(|pou| matches!(pou, Pou::FunctionBlock(fb) if fb.name == "TON"))
            .count();
        assert_eq!(ton_count, 1);
    }

    #[test]
    fn time_literals() {
        assert_eq!(parse_time_literal("T#0ms"), Some(0));
        assert_eq!(parse_time_literal("T#500ms"), Some(500));
        assert_eq!(parse_time_literal("T#5s"), Some(5_000));
        assert_eq!(parse_time_literal("t#2m"), Some(120_000));
        assert_eq!(parse_time_literal("TIME#1h30m"), Some(5_400_000));
        assert_eq!(parse_time_literal("T#1d"), Some(86_400_000));
        assert_eq!(parse_time_literal("T#2m10s500ms"), Some(130_500));
        assert_eq!(parse_time_literal("T#1_000ms"), Some(1_000));
        assert_eq!(parse_time_literal("T#-2s"), Some(-2_000));
        assert_eq!(parse_time_literal("T#1.5s"), Some(1_500));
        assert_eq!(parse_time_literal("T#250"), Some(250));
    }

    #[test]
    fn rejects_malformed_time_literals() {
        assert_eq!(parse_time_literal("T#"), None);
        assert_eq!(parse_time_literal("T#5q"), None);
        assert_eq!(parse_time_literal("T#s"), None);
    }
}
