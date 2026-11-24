// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-cli.md, section "main.rs"
// ============================================================================

//! The gindema CLI: assembles, builds, remaps, runs, and inspects
//! gindema-vm bytecode program files, and packs/unpacks gindema-container
//! asset archives. See `docs/gindema-cli.md` for what this does and -- just
//! as importantly -- does not do yet (no build-pipeline integration, and
//! `gindema-guard` isn't used here at all; see that doc's "Known limits"
//! for why).

mod cli;
mod commands;

use cli::{parse_args, Command, USAGE};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let command = match parse_args(&args) {
        Ok(command) => command,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!();
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    };

    let result = match command {
        Command::Help => {
            print!("{USAGE}");
            Ok(())
        }
        Command::Demo {
            output,
            honeypots,
            seed,
        } => commands::run_demo(&output, honeypots, seed),
        Command::Info { input } => commands::run_info(&input),
        Command::Run { input } => commands::run_run(&input),
        Command::Remap {
            input,
            output,
            seed,
        } => commands::run_remap(&input, &output, seed),
        Command::Assemble { source, output } => commands::run_assemble(&source, &output),
        Command::Pack {
            output,
            inputs,
            honeypots,
            seed,
        } => commands::run_pack(&output, &inputs, honeypots, seed),
        Command::Unpack {
            input,
            key,
            output_dir,
        } => commands::run_unpack(&input, &key, &output_dir),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
