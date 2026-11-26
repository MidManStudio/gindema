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
    /// Control-flow flattening: loads `input`, rebuilds it as a single
    /// `seed`-derived dispatcher loop (see
    /// `gindema_obfuscate::flatten_control_flow`), writes the result to
    /// `output`.
    Flatten {
        input: PathBuf,
        output: PathBuf,
        seed: u64,
    },
    /// State-dependent decoding: loads `input`, interleaves `seed`-derived
    /// fluff bits between every instruction (see
    /// `gindema_obfuscate::obfuscate_with_fluff`), writes the result to
    /// `output`.
    Fluff {
        input: PathBuf,
        output: PathBuf,
        seed: u64,
    },
    /// Compiles a gindema-vm assembly text file (see
    /// `gindema_vm::assemble`) into a program file.
    Assemble { source: PathBuf, output: PathBuf },
    /// Packs one or more files into a new gindema-container archive (see
    /// `commands::pack_assets`), generating and printing a fresh key.
    Pack {
        output: PathBuf,
        inputs: Vec<PathBuf>,
        honeypots: u32,
        seed: u64,
    },
    /// Opens a gindema-container archive with a given key and extracts
    /// every real asset into a directory.
    Unpack {
        input: PathBuf,
        key: String,
        output_dir: PathBuf,
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
        "remap" => {
            parse_input_output_seed(rest, "remap").map(|(input, output, seed)| Command::Remap {
                input,
                output,
                seed,
            })
        }
        "flatten" => {
            parse_input_output_seed(rest, "flatten").map(|(input, output, seed)| Command::Flatten {
                input,
                output,
                seed,
            })
        }
        "fluff" => {
            parse_input_output_seed(rest, "fluff").map(|(input, output, seed)| Command::Fluff {
                input,
                output,
                seed,
            })
        }
        "assemble" => parse_two_paths(rest, "assemble")
            .map(|(source, output)| Command::Assemble { source, output }),
        "pack" => parse_pack(rest),
        "unpack" => parse_unpack(rest),
        other => Err(err(format!("unknown subcommand '{other}' (try 'help')"))),
    }
}

fn parse_two_paths(rest: &[String], name: &str) -> Result<(PathBuf, PathBuf), ArgsError> {
    match rest {
        [a, b] => Ok((PathBuf::from(a), PathBuf::from(b))),
        _ => Err(err(format!(
            "'{name}' needs exactly an input and an output file path"
        ))),
    }
}

fn parse_pack(rest: &[String]) -> Result<Command, ArgsError> {
    let mut positionals = Vec::new();
    let mut honeypots = 0u32;
    let mut seed = 0u64;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--honeypots" => honeypots = parse_u64_flag(&mut iter, "--honeypots")? as u32,
            "--seed" => seed = parse_u64_flag(&mut iter, "--seed")?,
            _ if arg.starts_with("--") => {
                return Err(err(format!("unknown flag '{arg}' for 'pack'")))
            }
            path => positionals.push(path),
        }
    }
    let [output, inputs @ ..] = positionals.as_slice() else {
        return Err(err(
            "'pack' needs an output path and at least one input file",
        ));
    };
    if inputs.is_empty() {
        return Err(err("'pack' needs at least one input file"));
    }
    Ok(Command::Pack {
        output: PathBuf::from(output),
        inputs: inputs.iter().map(PathBuf::from).collect(),
        honeypots,
        seed,
    })
}

fn parse_unpack(rest: &[String]) -> Result<Command, ArgsError> {
    let mut positionals = Vec::new();
    let mut key = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--key" => {
                key = Some(
                    iter.next()
                        .ok_or_else(|| err("--key needs a value"))?
                        .clone(),
                )
            }
            _ if arg.starts_with("--") => {
                return Err(err(format!("unknown flag '{arg}' for 'unpack'")))
            }
            path => positionals.push(path),
        }
    }
    let [input, output_dir] = positionals.as_slice() else {
        return Err(err(
            "'unpack' needs exactly an input archive and an output directory",
        ));
    };
    let key = key.ok_or_else(|| err("'unpack' needs --key <BASE64>"))?;
    Ok(Command::Unpack {
        input: PathBuf::from(input),
        key,
        output_dir: PathBuf::from(output_dir),
    })
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

/// Shared by `remap`, `flatten`, and `fluff` — all three take exactly the
/// same shape (`<input> <output> --seed <S>`) and differ only in which
/// `gindema-vm`/`gindema-obfuscate` function `commands.rs` goes on to call
/// with the result.
fn parse_input_output_seed(
    rest: &[String],
    name: &str,
) -> Result<(PathBuf, PathBuf, u64), ArgsError> {
    let mut positionals = Vec::new();
    let mut seed = None;
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--seed" => seed = Some(parse_u64_flag(&mut iter, "--seed")?),
            _ if arg.starts_with("--") => {
                return Err(err(format!("unknown flag '{arg}' for '{name}'")))
            }
            path => positionals.push(path),
        }
    }
    let [input, output] = positionals.as_slice() else {
        return Err(err(format!(
            "'{name}' needs exactly an input and an output file path"
        )));
    };
    let seed = seed.ok_or_else(|| err(format!("'{name}' needs --seed <number>")))?;
    Ok((PathBuf::from(input), PathBuf::from(output), seed))
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
gindema-cli: assemble, build, remap, flatten, fluff, run, and inspect
gindema-vm bytecode programs, and pack/unpack gindema-container asset
archives.

'demo' still builds a fixed built-in program; 'assemble' compiles an
arbitrary gindema-vm assembly text file instead (see docs/gindema-cli.md
and gindema_vm::assemble for the syntax). 'flatten' and 'fluff' are
gindema-obfuscate's other two passes, alongside 'remap' -- apply them in
that order (flatten, then fluff, then remap) for a correct pipeline; see
docs/gindema-obfuscate.md for why.

USAGE:
    gindema-cli demo <output> [--honeypots N] [--seed S]
    gindema-cli assemble <source> <output>
    gindema-cli info <input>
    gindema-cli run <input>
    gindema-cli remap <input> <output> --seed <S>
    gindema-cli flatten <input> <output> --seed <S>
    gindema-cli fluff <input> <output> --seed <S>
    gindema-cli pack <output> <input>... [--honeypots N] [--seed S]
    gindema-cli unpack <input> --key <BASE64> <output-dir>
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
    fn flatten_needs_two_paths_and_a_seed() {
        let args = vec![
            "flatten".to_string(),
            "in.gvm".to_string(),
            "out.gvm".to_string(),
            "--seed".to_string(),
            "7".to_string(),
        ];
        assert_eq!(
            parse_args(&args).unwrap(),
            Command::Flatten {
                input: PathBuf::from("in.gvm"),
                output: PathBuf::from("out.gvm"),
                seed: 7,
            }
        );
    }

    #[test]
    fn flatten_without_seed_errors() {
        let args = vec![
            "flatten".to_string(),
            "in.gvm".to_string(),
            "out.gvm".to_string(),
        ];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn fluff_needs_two_paths_and_a_seed() {
        let args = vec![
            "fluff".to_string(),
            "in.gvm".to_string(),
            "out.gvm".to_string(),
            "--seed".to_string(),
            "99".to_string(),
        ];
        assert_eq!(
            parse_args(&args).unwrap(),
            Command::Fluff {
                input: PathBuf::from("in.gvm"),
                output: PathBuf::from("out.gvm"),
                seed: 99,
            }
        );
    }

    #[test]
    fn fluff_without_seed_errors() {
        let args = vec![
            "fluff".to_string(),
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

    #[test]
    fn assemble_needs_exactly_a_source_and_an_output_path() {
        assert_eq!(
            parse_args(&[
                "assemble".to_string(),
                "prog.gasm".to_string(),
                "prog.gvm".to_string(),
            ])
            .unwrap(),
            Command::Assemble {
                source: PathBuf::from("prog.gasm"),
                output: PathBuf::from("prog.gvm"),
            }
        );
        assert!(parse_args(&["assemble".to_string(), "prog.gasm".to_string()]).is_err());
        assert!(parse_args(&["assemble".to_string()]).is_err());
    }

    #[test]
    fn pack_with_no_flags_and_one_input() {
        let args = vec![
            "pack".to_string(),
            "out.gctn".to_string(),
            "asset.bin".to_string(),
        ];
        assert_eq!(
            parse_args(&args).unwrap(),
            Command::Pack {
                output: PathBuf::from("out.gctn"),
                inputs: vec![PathBuf::from("asset.bin")],
                honeypots: 0,
                seed: 0,
            }
        );
    }

    #[test]
    fn pack_with_multiple_inputs_and_flags() {
        let args = vec![
            "pack".to_string(),
            "out.gctn".to_string(),
            "a.bin".to_string(),
            "b.bin".to_string(),
            "--honeypots".to_string(),
            "4".to_string(),
            "--seed".to_string(),
            "7".to_string(),
        ];
        assert_eq!(
            parse_args(&args).unwrap(),
            Command::Pack {
                output: PathBuf::from("out.gctn"),
                inputs: vec![PathBuf::from("a.bin"), PathBuf::from("b.bin")],
                honeypots: 4,
                seed: 7,
            }
        );
    }

    #[test]
    fn pack_missing_output_or_inputs_errors() {
        assert!(parse_args(&["pack".to_string()]).is_err());
        assert!(parse_args(&["pack".to_string(), "out.gctn".to_string()]).is_err());
    }

    #[test]
    fn unpack_needs_a_key_and_two_paths() {
        let args = vec![
            "unpack".to_string(),
            "archive.gctn".to_string(),
            "--key".to_string(),
            "c29tZS1rZXk=".to_string(),
            "out-dir".to_string(),
        ];
        assert_eq!(
            parse_args(&args).unwrap(),
            Command::Unpack {
                input: PathBuf::from("archive.gctn"),
                key: "c29tZS1rZXk=".to_string(),
                output_dir: PathBuf::from("out-dir"),
            }
        );
    }

    #[test]
    fn unpack_without_key_errors() {
        let args = vec![
            "unpack".to_string(),
            "archive.gctn".to_string(),
            "out-dir".to_string(),
        ];
        assert!(parse_args(&args).is_err());
    }
}
