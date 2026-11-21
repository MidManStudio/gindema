# gindema-cli

## Overview

The distributable protection-core CLI: builds, remaps, runs, and inspects
gindema-vm bytecode program files. This first pass is a real, working
pipeline — `demo` → `remap` → `run`/`info` — not a stub, but it has one
load-bearing limitation worth stating up front: **there is no assembly
language or general-purpose compiler front end yet**, anywhere in the
workspace. `gindema-vm` programs are built by calling `ProgramBuilder`
methods directly, in Rust. So this CLI's `demo` subcommand builds one
fixed, hardcoded program — not whatever the user wants — which is what
every other subcommand then has something real to operate on.

Ties together `gindema-vm` (`Program`, `ProgramBuilder`, `Vm`,
`Program::to_bytes`/`from_bytes`) and `gindema-obfuscate`
(`inject_honeypot`, `remap_opcodes`). Does **not** use `gindema-guard` or
`gindema-container` — see "Known limits" for why neither fits this crate's
actual job, rather than them simply being forgotten.

## Subcommands

```
gindema-cli demo <output> [--honeypots N] [--seed S]
gindema-cli info <input>
gindema-cli run <input>
gindema-cli remap <input> <output> --seed <S>
gindema-cli help
```

- **`demo`** builds the fixed demo program (sums `1..=10` via a real loop —
  always `55` — see `commands.rs` below), optionally injecting up to 3
  honeypots (`--honeypots`, seeded from `--seed`), and writes it to
  `output` via `Program::to_bytes`.
- **`info`** loads a program file and prints its bytecode length,
  constant count, local count, and whether its opcode table is the
  identity mapping or a remap — without running it.
- **`run`** loads and runs a program file through a fresh `Vm`, printing
  whatever's left on the stack.
- **`remap`** loads a program file, calls `gindema_obfuscate::remap_opcodes`
  with the given seed, and writes the result to a new file. The output is
  the same length and behaves identically — only its opcode numbering
  differs; see `docs/gindema-vm.md` and `docs/gindema-obfuscate.md` for why
  that's guaranteed rather than merely expected.

## Modules

### `main.rs`
**What it does:** crate doc, parses arguments via `cli::parse_args`,
dispatches to `commands::run_*`, prints `cli::USAGE` for `help` or a parse
error, exits `2` on a bad invocation and `1` on a command that ran but
failed (file not found, a malformed program file, a runtime error) — the
usual Unix convention of distinguishing "you used me wrong" from "something
you asked for didn't work."

### `cli.rs`
**What it does:** `Command` (one variant per subcommand, including
`Help`), `parse_args(&[String]) -> Result<Command, ArgsError>`, and
`USAGE`. Deliberately hand-rolled rather than built on a crate like `clap`:
four subcommands with a couple of positional paths and two optional `--seed`/
`--honeypots` flags between them doesn't need a general-purpose argument
parser, and keeping this crate's own dependency footprint small felt like
the right trade for that little parsing logic. Kept entirely separate from
actually running a command (`commands.rs`) specifically so parsing itself
is unit-testable without touching the filesystem — every test in this file
calls `parse_args` directly and checks the resulting `Command` or that it's
an `Err`, never spawning the actual binary.

**Tests:** inline. No args or `help`/`--help` all give `Command::Help`;
`demo` with no flags defaults `honeypots`/`seed` to `0`; `demo` with both
flags in either order; `demo` missing its output path errors; `info`/`run`
each require exactly one path (zero or two both error); `remap` needs both
paths and `--seed`, and errors without the seed; an unknown subcommand and
an unknown flag both error; a non-numeric `--seed` value errors instead of
panicking on the `.parse::<u64>()` call.

### `commands.rs`
**What it does:** `build_demo_program(honeypot_count, seed) -> Program` —
the one fixed demo program (2 locals: a running sum and a counter; sums
`1..=10` via `Lt`/`JumpIfFalse`/`Jump`, so there's a real loop and real
jump targets for the rest of the pipeline to exercise, not just straight-
line arithmetic) — plus `run_demo`/`run_info`/`run_run`/`run_remap`, each
doing real file I/O (`std::fs::read`/`write`) and returning
`Result<(), Box<dyn Error>>`. `Box<dyn Error>` rather than a crate-specific
enum: this is the actual end of the pipeline (nothing calls these functions
except `main`, which only ever needs to print the error and set an exit
code), so there's no caller that would benefit from matching on a specific
variant the way `VmError`/`BuildError`/`SerializeError`/`ObfuscateError`
all need to be matchable deeper in the stack.

**Tests:** inline, all pure (no filesystem access — `run_*`'s file I/O
isn't itself unit tested, only `build_demo_program` and the library calls
it feeds into). The demo program with zero honeypots sums to `55`; with
all 3 honeypot slots filled, still `55`; more honeypots means a strictly
larger program (not a no-op); the same `(honeypot_count, seed)` pair gives
byte-identical output; the demo program round-trips through
`to_bytes`/`from_bytes` and still runs correctly; remapping the demo
program (the one with a honeypot included) preserves its behavior — this
last test is the one that actually caught the real `gindema-vm` bug
described below, the first time it ran.

## Known limits (intentional, for this pass)

- **No assembly language or compiler front end.** `demo` is the only way
  to get a program file at all; there is no way to express an arbitrary
  program from the command line. This is the single biggest gap between
  what this crate's name promises ("the distributable protection-core
  CLI") and what it does today.
- **`gindema-guard` isn't used here, on purpose.** Its primitives
  (`TimingWindow`, `Watch`, `GuardState`, `Guarded<T>`) are meant to run
  *inside a shipped game* watching the game's own runtime state — there
  is no game in this repository for a dev-time CLI tool to protect, and
  forcing an artificial integration (having the CLI watch its own output
  file, say) wouldn't demonstrate anything the crate's own tests don't
  already cover more directly.
- **`gindema-container` isn't used here either.** It's still an
  unimplemented scaffold stub (see `docs/gindema-container.md`, not yet
  written) — there's no real asset-packing logic to call yet. `gindema-cli`
  already declares the dependency for when that changes.
- **`gindema-ptr`/`gindema-bits` are transitively present but not used
  directly** — `gindema-vm` and `gindema-obfuscate` already use `gindema-
  bits`; nothing in this crate's own code needs either directly.
- **No build-pipeline integration beyond this CLI itself.** Running
  `remap` by hand with a manually-chosen seed is the whole story right
  now; nothing automatically picks a fresh seed per build or calls this as
  part of a packaging step.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-cli` runs 15 unit tests; `cargo clippy -p
gindema-cli --all-targets -- -D warnings` and `cargo fmt -p gindema-cli --
--check` are both clean. Beyond the unit tests, the actual built binary
was run end-to-end by hand in the sandbox — `demo --honeypots 3`, `info`,
`run` (got `Int(55)`), `remap --seed 777`, `info` on the remapped file
(confirmed `opcode map: remapped (non-identity)`), `run` on the remapped
file (got `Int(55)` again), and `run` on a nonexistent file (exit code 1,
clear error on stderr) — not just `cargo test`. Verified in the sandbox on
Rust 1.100; building this crate also compiles `gindema-container` and its
`dixscript` dependency transitively (confirmed working in the sandbox),
though neither is separately linted or tested here per the workspace's
standing DixScript exclusion.

## Fixes and Problems

- The first real run of `commands::tests::remapping_the_demo_program_preserves_its_behavior`
  (a demo program built with one honeypot, then remapped) failed with
  `InvalidOpcode(0)` — not a bug in this crate, but the first thing in the
  whole workspace to actually exercise "remap a program that contains a
  honeypot," which surfaced a real bug in `gindema-vm`'s `remap_program`
  (it stopped decoding at the honeypot's own dead-end `Halt` instead of the
  program's real one). Fixed in `gindema-vm` — see that crate's own "Fixes
  and Problems" for the full story — after which this test, and the rest
  of this crate's suite, passed without further changes needed here. Left
  as a visible example of integration testing doing its job: a bug that
  three different crates' own unit tests, each passing in isolation,
  didn't catch on its own.
