// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "asm.rs"
// ============================================================================

//! A minimal textual assembly language for [`crate::Program`], so a program
//! can be written as a text file instead of only through
//! [`crate::ProgramBuilder`]'s Rust API. This was the single biggest gap
//! flagged in `gindema-cli`'s own docs ("no way to express an arbitrary
//! program from the command line") -- [`assemble`] is what closes it.
//!
//! # Syntax
//!
//! - One instruction per line. `;` starts a comment that runs to the end of
//!   the line; blank lines are ignored.
//! - `.locals N` sets the program's local-slot count (see
//!   [`crate::ProgramBuilder::new`]); omitted, it defaults to `0`. At most
//!   one `.locals` directive per source.
//! - `name:` defines a label at the next instruction (or at the end of the
//!   program, if nothing follows it) -- forward references resolve the same
//!   as backward ones, since every label name is collected before any
//!   instruction is emitted. A label and an instruction can share one line
//!   (`top: load_local 0`).
//! - One mnemonic per remaining instruction line, matching
//!   [`crate::ProgramBuilder`]'s emitter methods one-to-one: `halt`, `pop`,
//!   `dup`, `swap`, `add`, `sub`, `mul`, `div`, `neg`, `not`, `eq`, `lt` (no
//!   operand); `load_const <literal>` (an integer, a float containing `.`,
//!   or `true`/`false` -- added to the constant pool fresh each time, see
//!   "Known limits" in `docs/gindema-vm.md`); `load_local <slot>` /
//!   `store_local <slot>` (a `u8` slot index); `jump <label>` /
//!   `jump_if_false <label>` (a label name defined somewhere in the same
//!   source).
//!
//! # Example
//!
//! ```
//! use gindema_vm::{assemble, Value, Vm};
//!
//! // Same program as gindema-cli's built-in demo: sums 1..=10 via a loop.
//! let source = "
//!     .locals 2
//!     load_const 0
//!     store_local 0
//!     load_const 1
//!     store_local 1
//! top:
//!     load_local 1
//!     load_const 11
//!     lt
//!     jump_if_false end
//!     load_local 0
//!     load_local 1
//!     add
//!     store_local 0
//!     load_local 1
//!     load_const 1
//!     add
//!     store_local 1
//!     jump top
//! end:
//!     load_local 0
//!     halt
//! ";
//! let program = assemble(source).unwrap();
//! let mut vm = Vm::new();
//! assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(55)));
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::program::{BuildError, LabelId, Program, ProgramBuilder};
use crate::value::Value;

/// Everything that can go wrong assembling gindema-vm assembly text into a
/// [`Program`]. Line numbers are 1-based, matching how a text editor numbers
/// them; label errors have no line number since a label's identity is its
/// name, not where it happens to be written.
///
/// Variants carry plain tuple fields (rather than named struct fields) so
/// `missing_docs` doesn't require a doc comment per field -- consistent with
/// every other error enum in this crate ([`BuildError`],
/// [`crate::SerializeError`]). Each variant's own doc comment below states
/// what its fields mean, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsmError {
    /// A `.` directive this assembler doesn't know (only `.locals` is
    /// defined). Fields: line number, the unknown directive's name.
    UnknownDirective(usize, String),
    /// A directive (currently only `.locals`) appeared more than once.
    /// Fields: line number, the directive's name.
    DuplicateDirective(usize, String),
    /// An instruction mnemonic this assembler doesn't know. Fields: line
    /// number, the unknown mnemonic text.
    UnknownMnemonic(usize, String),
    /// The same label name (`name:`) was defined more than once. Field: the
    /// label's name.
    DuplicateLabel(String),
    /// `jump`/`jump_if_false` referenced a label with no matching `name:`
    /// definition anywhere in the source. Field: the label's name.
    UndefinedLabel(String),
    /// An instruction that needs an operand didn't get one. Fields: line
    /// number, the mnemonic.
    MissingOperand(usize, String),
    /// An instruction got more operands than it takes (every mnemonic here
    /// takes at most one). Fields: line number, the mnemonic.
    TooManyOperands(usize, String),
    /// An operand's text didn't parse as what the mnemonic expects (an
    /// integer, a float, or a `true`/`false` literal for `load_const`; a
    /// `u8` slot index for `load_local`/`store_local`; a `u8` count for
    /// `.locals`). Fields: line number, the mnemonic, the offending text.
    InvalidOperand(usize, String, String),
    /// Propagated from [`ProgramBuilder::build`]/[`ProgramBuilder::add_const`]
    /// -- in practice only [`BuildError::TooManyConstants`] or
    /// [`BuildError::ProgramTooLarge`], since this assembler resolves every
    /// label itself before calling `build` (see "Fixes and Problems" in
    /// `docs/gindema-vm.md` for why `UnresolvedLabel` specifically can't
    /// happen here).
    Build(BuildError),
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDirective(line, text) => {
                write!(f, "line {line}: unknown directive '.{text}'")
            }
            Self::DuplicateDirective(line, name) => {
                write!(f, "line {line}: '{name}' given more than once")
            }
            Self::UnknownMnemonic(line, text) => {
                write!(f, "line {line}: unknown instruction '{text}'")
            }
            Self::DuplicateLabel(name) => {
                write!(f, "label '{name}' is defined more than once")
            }
            Self::UndefinedLabel(name) => write!(f, "jump to undefined label '{name}'"),
            Self::MissingOperand(line, mnemonic) => {
                write!(f, "line {line}: '{mnemonic}' needs an operand")
            }
            Self::TooManyOperands(line, mnemonic) => {
                write!(f, "line {line}: too many operands for '{mnemonic}'")
            }
            Self::InvalidOperand(line, mnemonic, text) => write!(
                f,
                "line {line}: '{mnemonic}' can't use '{text}' as an operand"
            ),
            Self::Build(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AsmError {}

impl From<BuildError> for AsmError {
    fn from(e: BuildError) -> Self {
        Self::Build(e)
    }
}

/// One already-parsed line of source, past comment-stripping and the
/// `label:`/directive split -- everything after this point works on
/// structure, not raw text.
enum ParsedLine<'a> {
    Label(&'a str),
    Instr {
        mnemonic: &'a str,
        operand: Option<&'a str>,
        line: usize,
    },
}

/// Assembles `source` into a [`Program`]. See the module docs above for the
/// syntax. Every error path reports a 1-based line number (for line-scoped
/// problems) or the offending label name (for label problems), never a
/// panic -- a hand-written or generated `.gasm` file is handled the same
/// way a corrupted one would be.
pub fn assemble(source: &str) -> Result<Program, AsmError> {
    let mut locals: Option<u8> = None;
    let mut parsed: Vec<ParsedLine> = Vec::new();

    for (zero_based, raw_line) in source.lines().enumerate() {
        let line_no = zero_based + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix('.') {
            parse_directive(rest, line_no, &mut locals)?;
            continue;
        }

        let (label, rest) = match line.split_once(':') {
            Some((name, rest)) if is_identifier(name.trim()) => (Some(name.trim()), rest.trim()),
            _ => (None, line),
        };
        if let Some(name) = label {
            parsed.push(ParsedLine::Label(name));
        }
        if rest.is_empty() {
            continue;
        }

        let mut tokens = rest.split_whitespace();
        let mnemonic = tokens
            .next()
            .expect("rest is non-empty, so split_whitespace yields at least one token");
        let operand = tokens.next();
        if tokens.next().is_some() {
            return Err(AsmError::TooManyOperands(line_no, mnemonic.to_string()));
        }
        parsed.push(ParsedLine::Instr {
            mnemonic,
            operand,
            line: line_no,
        });
    }

    // Every label name is collected -- and every jump target checked
    // against that collection -- before a single `ProgramBuilder` call is
    // made, so a label can be referenced before its `name:` definition
    // appears in the file, and `ProgramBuilder::build`'s own
    // `UnresolvedLabel` error (every label this assembler creates is always
    // placed) can never actually occur from assembled source.
    let mut defined: HashSet<&str> = HashSet::new();
    for parsed_line in &parsed {
        if let ParsedLine::Label(name) = parsed_line {
            if !defined.insert(*name) {
                return Err(AsmError::DuplicateLabel((*name).to_string()));
            }
        }
    }
    for parsed_line in &parsed {
        if let ParsedLine::Instr {
            mnemonic,
            operand,
            line,
        } = parsed_line
        {
            if matches!(*mnemonic, "jump" | "jump_if_false") {
                let name = require_operand(*operand, mnemonic, *line)?;
                if !defined.contains(name) {
                    return Err(AsmError::UndefinedLabel(name.to_string()));
                }
            }
        }
    }

    let mut builder = ProgramBuilder::new(locals.unwrap_or(0));
    let mut labels: HashMap<&str, LabelId> = HashMap::with_capacity(defined.len());
    for name in &defined {
        labels.insert(name, builder.new_label());
    }

    for parsed_line in &parsed {
        match parsed_line {
            ParsedLine::Label(name) => {
                builder.place_label(labels[name]);
            }
            ParsedLine::Instr {
                mnemonic,
                operand,
                line,
            } => emit(&mut builder, &labels, mnemonic, *operand, *line)?,
        }
    }

    Ok(builder.build()?)
}

fn parse_directive(rest: &str, line: usize, locals: &mut Option<u8>) -> Result<(), AsmError> {
    let mut tokens = rest.split_whitespace();
    let name = tokens.next().unwrap_or_default();
    match name {
        "locals" => {
            let text = tokens
                .next()
                .ok_or_else(|| AsmError::MissingOperand(line, ".locals".to_string()))?;
            if tokens.next().is_some() {
                return Err(AsmError::TooManyOperands(line, ".locals".to_string()));
            }
            if locals.is_some() {
                return Err(AsmError::DuplicateDirective(line, ".locals".to_string()));
            }
            *locals = Some(parse_u8(text, ".locals", line)?);
            Ok(())
        }
        other => Err(AsmError::UnknownDirective(line, other.to_string())),
    }
}

fn emit(
    b: &mut ProgramBuilder,
    labels: &HashMap<&str, LabelId>,
    mnemonic: &str,
    operand: Option<&str>,
    line: usize,
) -> Result<(), AsmError> {
    macro_rules! no_operand {
        ($method:ident) => {{
            require_no_operand(operand, mnemonic, line)?;
            b.$method();
        }};
    }

    match mnemonic {
        "halt" => no_operand!(halt),
        "pop" => no_operand!(pop),
        "dup" => no_operand!(dup),
        "swap" => no_operand!(swap),
        "add" => no_operand!(add),
        "sub" => no_operand!(sub),
        "mul" => no_operand!(mul),
        "div" => no_operand!(div),
        "neg" => no_operand!(neg),
        "not" => no_operand!(not),
        "eq" => no_operand!(eq),
        "lt" => no_operand!(lt),
        "load_const" => {
            let text = require_operand(operand, mnemonic, line)?;
            let value = parse_literal(text, mnemonic, line)?;
            let idx = b.add_const(value)?;
            b.load_const(idx);
        }
        "load_local" => {
            let text = require_operand(operand, mnemonic, line)?;
            b.load_local(parse_u8(text, mnemonic, line)?);
        }
        "store_local" => {
            let text = require_operand(operand, mnemonic, line)?;
            b.store_local(parse_u8(text, mnemonic, line)?);
        }
        "jump" => {
            let text = require_operand(operand, mnemonic, line)?;
            b.jump(labels[text]);
        }
        "jump_if_false" => {
            let text = require_operand(operand, mnemonic, line)?;
            b.jump_if_false(labels[text]);
        }
        other => return Err(AsmError::UnknownMnemonic(line, other.to_string())),
    }
    Ok(())
}

fn require_no_operand(operand: Option<&str>, mnemonic: &str, line: usize) -> Result<(), AsmError> {
    if operand.is_some() {
        Err(AsmError::TooManyOperands(line, mnemonic.to_string()))
    } else {
        Ok(())
    }
}

fn require_operand<'a>(
    operand: Option<&'a str>,
    mnemonic: &str,
    line: usize,
) -> Result<&'a str, AsmError> {
    operand.ok_or_else(|| AsmError::MissingOperand(line, mnemonic.to_string()))
}

fn parse_literal(text: &str, mnemonic: &str, line: usize) -> Result<Value, AsmError> {
    if text == "true" {
        return Ok(Value::Bool(true));
    }
    if text == "false" {
        return Ok(Value::Bool(false));
    }
    let invalid = || AsmError::InvalidOperand(line, mnemonic.to_string(), text.to_string());
    if text.contains('.') {
        text.parse::<f64>().map(Value::Float).map_err(|_| invalid())
    } else {
        text.parse::<i64>().map(Value::Int).map_err(|_| invalid())
    }
}

fn parse_u8(text: &str, mnemonic: &str, line: usize) -> Result<u8, AsmError> {
    text.parse::<u8>()
        .map_err(|_| AsmError::InvalidOperand(line, mnemonic.to_string(), text.to_string()))
}

fn strip_comment(line: &str) -> &str {
    match line.find(';') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::Vm;

    #[test]
    fn assembles_the_lib_doc_example_expression() {
        let program = assemble(
            "
            load_const 2
            load_const 3
            add
            load_const 4
            mul
            halt
            ",
        )
        .unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(20)));
    }

    #[test]
    fn assembles_a_loop_summing_one_through_ten() {
        let source = "
            .locals 2
            load_const 0
            store_local 0
            load_const 1
            store_local 1
        top:
            load_local 1
            load_const 11
            lt
            jump_if_false end
            load_local 0
            load_local 1
            add
            store_local 0
            load_local 1
            load_const 1
            add
            store_local 1
            jump top
        end:
            load_local 0
            halt
        ";
        let program = assemble(source).unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(55)));
    }

    #[test]
    fn forward_references_resolve_the_same_as_backward_ones() {
        // `end` is referenced before its `name:` definition appears.
        let program = assemble(
            "
            jump end
            load_const 1
            halt
        end:
            load_const 2
            halt
            ",
        )
        .unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(2)));
    }

    #[test]
    fn a_label_and_an_instruction_can_share_one_line() {
        let program = assemble("top: load_const 1\nhalt\n").unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(1)));
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let program = assemble(
            "
            ; this whole line is a comment

            load_const 7 ; trailing comment too
            halt
            ",
        )
        .unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(7)));
    }

    #[test]
    fn load_const_parses_int_float_and_bool_literals() {
        let program = assemble("load_const 3.5\nhalt\n").unwrap();
        assert_eq!(program.constants(), [Value::Float(3.5)]);

        let program = assemble("load_const true\nhalt\n").unwrap();
        assert_eq!(program.constants(), [Value::Bool(true)]);

        let program = assemble("load_const false\nhalt\n").unwrap();
        assert_eq!(program.constants(), [Value::Bool(false)]);
    }

    #[test]
    fn omitted_locals_directive_defaults_to_zero() {
        let program = assemble("halt\n").unwrap();
        assert_eq!(program.local_count(), 0);
    }

    #[test]
    fn locals_directive_sets_the_local_count() {
        let program = assemble(".locals 3\nhalt\n").unwrap();
        assert_eq!(program.local_count(), 3);
    }

    #[test]
    fn duplicate_locals_directive_is_rejected() {
        assert_eq!(
            assemble(".locals 1\n.locals 2\nhalt\n"),
            Err(AsmError::DuplicateDirective(2, ".locals".to_string()))
        );
    }

    #[test]
    fn unknown_directive_is_rejected() {
        assert_eq!(
            assemble(".bogus 1\nhalt\n"),
            Err(AsmError::UnknownDirective(1, "bogus".to_string()))
        );
    }

    #[test]
    fn unknown_mnemonic_is_rejected() {
        assert_eq!(
            assemble("frobnicate\n"),
            Err(AsmError::UnknownMnemonic(1, "frobnicate".to_string()))
        );
    }

    #[test]
    fn jump_to_undefined_label_is_rejected() {
        assert_eq!(
            assemble("jump nowhere\nhalt\n"),
            Err(AsmError::UndefinedLabel("nowhere".to_string()))
        );
    }

    #[test]
    fn duplicate_label_definition_is_rejected() {
        assert_eq!(
            assemble("top:\nhalt\ntop:\nhalt\n"),
            Err(AsmError::DuplicateLabel("top".to_string()))
        );
    }

    #[test]
    fn missing_operand_is_rejected() {
        assert_eq!(
            assemble("load_const\nhalt\n"),
            Err(AsmError::MissingOperand(1, "load_const".to_string()))
        );
    }

    #[test]
    fn too_many_operands_is_rejected_for_a_one_operand_mnemonic() {
        assert_eq!(
            assemble("load_const 1 2\nhalt\n"),
            Err(AsmError::TooManyOperands(1, "load_const".to_string()))
        );
    }

    #[test]
    fn an_operand_on_a_zero_operand_mnemonic_is_rejected() {
        assert_eq!(
            assemble("halt now\n"),
            Err(AsmError::TooManyOperands(1, "halt".to_string()))
        );
    }

    #[test]
    fn invalid_literal_and_slot_operands_are_rejected() {
        assert_eq!(
            assemble("load_const not-a-number\nhalt\n"),
            Err(AsmError::InvalidOperand(
                1,
                "load_const".to_string(),
                "not-a-number".to_string()
            ))
        );
        assert_eq!(
            assemble("load_local 999\nhalt\n"),
            Err(AsmError::InvalidOperand(
                1,
                "load_local".to_string(),
                "999".to_string()
            ))
        );
    }

    #[test]
    fn too_many_constants_propagates_as_a_build_error() {
        let mut source = String::new();
        // One more than `add_const` allows (its own test, in program.rs,
        // confirms the boundary is exactly u16::MAX + 1 successful calls).
        for _ in 0..=(u16::MAX as u32 + 1) {
            source.push_str("load_const 0\npop\n");
        }
        source.push_str("halt\n");
        assert_eq!(
            assemble(&source),
            Err(AsmError::Build(BuildError::TooManyConstants))
        );
    }

    #[test]
    fn assembled_program_round_trips_through_to_bytes_and_still_runs() {
        let program = assemble("load_const 42\nhalt\n").unwrap();
        let restored = Program::from_bytes(&program.to_bytes()).unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&restored).unwrap(), Some(Value::Int(42)));
    }
}
