// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-cli.md, section "commands.rs"
// ============================================================================

use std::error::Error;
use std::path::Path;

use gindema_obfuscate::{inject_honeypot, remap_opcodes};
use gindema_vm::{OpcodeMap, Program, ProgramBuilder, Value, Vm};

/// Builds the one fixed demo program this crate ships: sums `1..=10` via a
/// real loop (so there's a jump target worth exercising), into local slot
/// `0`. Always evaluates to `55`. `honeypot_count` sprinkles up to 3
/// honeypots (see `gindema_obfuscate::inject_honeypot`) at different points
/// in the program, each seeded from `seed` so the same `(honeypot_count,
/// seed)` pair always produces byte-identical output.
///
/// There is no assembly language or general-purpose compiler front end
/// yet (see `docs/gindema-cli.md`'s "Known limits") — this hardcoded
/// program exists so the rest of this tool (`info`/`run`/`remap`) has a
/// real, interesting (a loop, not just arithmetic) file to operate on
/// without one.
pub fn build_demo_program(honeypot_count: u32, seed: u64) -> Program {
    let mut b = ProgramBuilder::new(2); // local 0 = running sum, local 1 = counter
    let zero = b
        .add_const(Value::Int(0))
        .expect("a handful of constants never overflows a 16-bit pool");
    let one = b.add_const(Value::Int(1)).expect("same as above");
    let eleven = b.add_const(Value::Int(11)).expect("same as above"); // loop while counter < 11

    b.load_const(zero).store_local(0);
    b.load_const(one).store_local(1);

    if honeypot_count >= 1 {
        inject_honeypot(&mut b, seed).expect("same as above, for the honeypot's own constants");
    }

    let top = b.new_label();
    let end = b.new_label();
    b.place_label(top);
    b.load_local(1).load_const(eleven).lt();
    b.jump_if_false(end);
    b.load_local(0).load_local(1).add().store_local(0);

    if honeypot_count >= 2 {
        inject_honeypot(&mut b, seed.wrapping_add(1)).expect("same as above");
    }

    b.load_local(1).load_const(one).add().store_local(1);
    b.jump(top);
    b.place_label(end);

    if honeypot_count >= 3 {
        inject_honeypot(&mut b, seed.wrapping_add(2)).expect("same as above");
    }

    b.load_local(0);
    b.halt();
    b.build().expect("a fixed, hand-written program never hits an unresolved label or an oversized constant pool")
}

/// `demo <output>`: builds [`build_demo_program`] and writes it to
/// `output`.
pub fn run_demo(output: &Path, honeypot_count: u32, seed: u64) -> Result<(), Box<dyn Error>> {
    let program = build_demo_program(honeypot_count, seed);
    let bytes = program.to_bytes();
    std::fs::write(output, &bytes)?;
    println!(
        "wrote {} ({} bytes): sums 1..=10 via a real loop, {} honeypot(s), seed {seed}",
        output.display(),
        bytes.len(),
        honeypot_count,
    );
    println!(
        "running it now should give 55 -- verify with: gindema-cli run {}",
        output.display()
    );
    Ok(())
}

/// `info <input>`: loads a program and prints a summary without running it.
pub fn run_info(input: &Path) -> Result<(), Box<dyn Error>> {
    let bytes = std::fs::read(input)?;
    let program = Program::from_bytes(&bytes)?;
    let remapped = program.opcode_map() != &OpcodeMap::identity();
    println!("{}", input.display());
    println!("  bytecode:   {} bytes", program.code_len_bytes());
    println!("  constants:  {}", program.constants().len());
    println!("  locals:     {}", program.local_count());
    println!(
        "  opcode map: {}",
        if remapped {
            "remapped (non-identity)"
        } else {
            "identity"
        }
    );
    Ok(())
}

/// `run <input>`: loads and runs a program, printing its result.
pub fn run_run(input: &Path) -> Result<(), Box<dyn Error>> {
    let bytes = std::fs::read(input)?;
    let program = Program::from_bytes(&bytes)?;
    let mut vm = Vm::new();
    match vm.run(&program)? {
        Some(value) => println!("{value:?}"),
        None => println!("(no value left on the stack)"),
    }
    Ok(())
}

/// `remap <input> <output> --seed <seed>`: per-build opcode remap.
pub fn run_remap(input: &Path, output: &Path, seed: u64) -> Result<(), Box<dyn Error>> {
    let bytes = std::fs::read(input)?;
    let program = Program::from_bytes(&bytes)?;
    let remapped = remap_opcodes(&program, seed)?;
    let out_bytes = remapped.to_bytes();
    std::fs::write(output, &out_bytes)?;
    println!(
        "wrote {} ({} bytes), opcode table remapped with seed {seed} -- same behavior as {}",
        output.display(),
        out_bytes.len(),
        input.display(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_program_with_no_honeypots_sums_one_through_ten() {
        let program = build_demo_program(0, 0);
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(55)));
    }

    #[test]
    fn demo_program_with_every_honeypot_slot_filled_still_sums_to_the_same_answer() {
        let program = build_demo_program(3, 777);
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(55)));
    }

    #[test]
    fn more_honeypots_mean_a_larger_program() {
        let none = build_demo_program(0, 1);
        let three = build_demo_program(3, 1);
        assert!(three.code_len_bytes() > none.code_len_bytes());
    }

    #[test]
    fn same_honeypot_count_and_seed_gives_byte_identical_programs() {
        let a = build_demo_program(2, 123);
        let b = build_demo_program(2, 123);
        assert_eq!(a.code_bytes(), b.code_bytes());
    }

    #[test]
    fn demo_program_round_trips_through_to_bytes_and_still_runs_correctly() {
        let program = build_demo_program(1, 5);
        let restored = Program::from_bytes(&program.to_bytes()).unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&restored).unwrap(), Some(Value::Int(55)));
    }

    #[test]
    fn remapping_the_demo_program_preserves_its_behavior() {
        let program = build_demo_program(1, 5);
        let remapped = remap_opcodes(&program, 999).unwrap();
        assert_ne!(remapped.code_bytes(), program.code_bytes());
        let mut vm = Vm::new();
        assert_eq!(vm.run(&remapped).unwrap(), Some(Value::Int(55)));
    }
}
