// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-cli.md, section "cli.rs"
// ============================================================================

use std::path::PathBuf;

/// The parsed form of whatever the user typed on the command line — kept
/// separate from actually running a command (see `commands.rs`) so argument
/// parsing itself can be unit tested without touching the filesystem.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Builds the built-in demo program (see `commands.rs`) and writes it
    /// to `output`.
    Demo {
        output: PathBuf,
        honeypots: u32,
        seed: u64,
    },
    /// Prints a summary of a program file without running it.
    Info { input: PathBuf },
    /// Loads and runs a program file, printing its result.
    Run { input: PathBuf },
    /// Per-build opcode remap: loads `input`, re-encodes it under a
    /// `seed`-derived random opcode table, writes the result to `output`.
    Remap {
        input: PathBuf,
        output: PathBuf,
        seed: u64,
    },
    /// `--help`, `help`, or no arguments at all.
    Help,
}

/// What can go wrong turning raw command-line arguments into a [`Command`].
/// Always a plain message: this crate is the end of the line for errors —
/// nothing downstream needs to match on an enum variant, only print this
/// and exit with a nonzero status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgsError(pub String);

impl std::fmt::Display for ArgsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ArgsError {}

fn err(msg: impl Into<String>) -> ArgsError {
    ArgsError(msg.into())
}

/// Parses `args` (expected to already have the program name itself
/// stripped — pass `&std::env::args().collect::<Vec<_>>()[1..]`) into a
/// [`Command`].
pub fn parse_args(args: &[String]) -> Result<Command, ArgsError> {
    let Some((subcommand, rest)) = args.split_first() else {
        return Ok(Command::Help);
    };

    match subcommand.as_str() {
        "help" | "--help" | "-h" => Ok(Command::Help),
        "demo" => parse_demo(rest),
        "info" => parse_single_path(rest, "info").map(|input| Command::Info { input }),
        "run" => parse_single_path(rest, "run").map(|input| Command::Run { input }),
        "remap" => parse_remap(rest),
        other => Err(err(format!("unknown subcommand '{other}' (try 'help')"))),
    }
}

fn parse_single_path(rest: &[String], name: &str) -> Result<PathBuf, ArgsError> {
    match rest {
        [path] => Ok(PathBuf::from(path)),
        [] => Err(err(format!("'{name}' needs a file path"))),
        _ => Err(err(format!("'{name}' takes exactly one file path"))),
    }
}

fn parse_demo(rest: &[String]) -> Result<Command, ArgsError> {
    let (output, flags) = rest
        .split_first()
        .ok_or_else(|| err("'demo' needs an output file path"))?;
    let mut honeypots = 0u32;
    let mut seed = 0u64;
    let mut iter = flags.iter();
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--honeypots" => honeypots = parse_u64_flag(&mut iter, "--honeypots")? as u32,
            "--seed" => seed = parse_u64_flag(&mut iter, "--seed")?,
            other => return Err(err(format!("unknown flag '{other}' for 'demo'"))),
        }
    }
    Ok(Command::Demo {
        output: PathBuf::from(output),
        honeypots,
        seed,
    })
}

fn parse_remap(rest: &[String]) -> Result<Command, ArgsError> {
    let mut positionals = Vec::new();
    let mut seed = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--seed" => seed = Some(parse_u64_flag(&mut iter, "--seed")?),
            _ if arg.starts_with("--") => {
                return Err(err(format!("unknown flag '{arg}' for 'remap'")))
            }
            path => positionals.push(path),
        }
    }
    let [input, output] = positionals.as_slice() else {
        return Err(err(
            "'remap' needs exactly an input and an output file path",
        ));
    };
    let seed = seed.ok_or_else(|| err("'remap' needs --seed <number>"))?;
    Ok(Command::Remap {
        input: PathBuf::from(input),
        output: PathBuf::from(output),
        seed,
    })
}

fn parse_u64_flag<'a>(
    iter: &mut impl Iterator<Item = &'a String>,
    flag_name: &str,
) -> Result<u64, ArgsError> {
    let raw = iter
        .next()
        .ok_or_else(|| err(format!("{flag_name} needs a value")))?;
    raw.parse::<u64>()
        .map_err(|_| err(format!("{flag_name} wants a number, got '{raw}'")))
}

/// Printed by `Command::Help` and by a parse error that's about overall
/// usage rather than one bad flag.
pub const USAGE: &str = "\
gindema-cli: build, remap, run, and inspect gindema-vm bytecode programs.

There is no assembly language yet (see docs/gindema-cli.md) -- 'demo'
builds a fixed built-in program so the rest of this tool has something
real to work on.

USAGE:
    gindema-cli demo <output> [--honeypots N] [--seed S]
    gindema-cli info <input>
    gindema-cli run <input>
    gindema-cli remap <input> <output> --seed <S>
    gindema-cli help
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_args_or_help_gives_help() {
        assert_eq!(parse_args(&[]).unwrap(), Command::Help);
        assert_eq!(parse_args(&["help".to_string()]).unwrap(), Command::Help);
        assert_eq!(parse_args(&["--help".to_string()]).unwrap(), Command::Help);
    }

    #[test]
    fn demo_with_no_flags_defaults_to_zero() {
        let args = vec!["demo".to_string(), "out.gvm".to_string()];
        assert_eq!(
            parse_args(&args).unwrap(),
            Command::Demo {
                output: PathBuf::from("out.gvm"),
                honeypots: 0,
                seed: 0,
            }
        );
    }

    #[test]
    fn demo_with_both_flags_in_either_order() {
        let args = vec![
            "demo".to_string(),
            "out.gvm".to_string(),
            "--seed".to_string(),
            "99".to_string(),
            "--honeypots".to_string(),
            "2".to_string(),
        ];
        assert_eq!(
            parse_args(&args).unwrap(),
            Command::Demo {
                output: PathBuf::from("out.gvm"),
                honeypots: 2,
                seed: 99,
            }
        );
    }

    #[test]
    fn demo_missing_output_path_errors() {
        let args = vec!["demo".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn info_and_run_take_exactly_one_path() {
        assert_eq!(
            parse_args(&["info".to_string(), "x.gvm".to_string()]).unwrap(),
            Command::Info {
                input: PathBuf::from("x.gvm")
            }
        );
        assert_eq!(
            parse_args(&["run".to_string(), "x.gvm".to_string()]).unwrap(),
            Command::Run {
                input: PathBuf::from("x.gvm")
            }
        );
        assert!(parse_args(&["info".to_string()]).is_err());
        assert!(
            parse_args(&["info".to_string(), "a.gvm".to_string(), "b.gvm".to_string()]).is_err()
        );
    }

    #[test]
    fn remap_needs_two_paths_and_a_seed() {
        let args = vec![
            "remap".to_string(),
            "in.gvm".to_string(),
            "out.gvm".to_string(),
            "--seed".to_string(),
            "42".to_string(),
        ];
        assert_eq!(
            parse_args(&args).unwrap(),
            Command::Remap {
                input: PathBuf::from("in.gvm"),
                output: PathBuf::from("out.gvm"),
                seed: 42,
            }
        );
    }

    #[test]
    fn remap_without_seed_errors() {
        let args = vec![
            "remap".to_string(),
            "in.gvm".to_string(),
            "out.gvm".to_string(),
        ];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn unknown_subcommand_and_unknown_flag_both_error() {
        assert!(parse_args(&["bogus".to_string()]).is_err());
        assert!(parse_args(&[
            "demo".to_string(),
            "out.gvm".to_string(),
            "--nope".to_string()
        ])
        .is_err());
    }

    #[test]
    fn non_numeric_seed_errors_instead_of_panicking() {
        let args = vec![
            "demo".to_string(),
            "out.gvm".to_string(),
            "--seed".to_string(),
            "not-a-number".to_string(),
        ];
        assert!(parse_args(&args).is_err());
    }
}
