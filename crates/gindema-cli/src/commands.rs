// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-cli.md, section "commands.rs"
// ============================================================================

use std::error::Error;
use std::path::Path;

use gindema_container::{ContainerBuilder, ContainerError, ContainerKey, ContainerReader};
use gindema_obfuscate::{
    flatten_control_flow, inject_honeypot, obfuscate_with_fluff, remap_opcodes,
};
use gindema_vm::{assemble, OpcodeMap, Program, ProgramBuilder, Value, Vm};

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

/// `flatten <input> <output> --seed <seed>`: control-flow flattening (see
/// `gindema_obfuscate::flatten_control_flow`). Apply this *before*
/// `fluff`/`remap` in a pipeline, not after -- flattening always rebuilds
/// from scratch, so it would otherwise silently discard either one (see
/// `docs/gindema-obfuscate.md`'s "Known limits").
pub fn run_flatten(input: &Path, output: &Path, seed: u64) -> Result<(), Box<dyn Error>> {
    let bytes = std::fs::read(input)?;
    let program = Program::from_bytes(&bytes)?;
    let flattened = flatten_control_flow(&program, seed)?;
    let out_bytes = flattened.to_bytes();
    std::fs::write(output, &out_bytes)?;
    println!(
        "wrote {} ({} bytes, up from {}), flattened into a dispatcher loop with seed {seed} -- same behavior as {}",
        output.display(),
        out_bytes.len(),
        bytes.len(),
        input.display(),
    );
    Ok(())
}

/// `fluff <input> <output> --seed <seed>`: state-dependent decoding (see
/// `gindema_obfuscate::obfuscate_with_fluff`). Apply `flatten` first if
/// you want both -- fluffing then flattening would discard the fluff (see
/// `run_flatten`'s own doc comment above).
pub fn run_fluff(input: &Path, output: &Path, seed: u64) -> Result<(), Box<dyn Error>> {
    let bytes = std::fs::read(input)?;
    let program = Program::from_bytes(&bytes)?;
    let fluffed = obfuscate_with_fluff(&program, seed)?;
    let out_bytes = fluffed.to_bytes();
    std::fs::write(output, &out_bytes)?;
    println!(
        "wrote {} ({} bytes, up from {}), fluffed with seed {seed} -- same behavior as {}",
        output.display(),
        out_bytes.len(),
        bytes.len(),
        input.display(),
    );
    Ok(())
}

/// `assemble <source> <output>`: compiles a gindema-vm assembly text file
/// (see `gindema_vm::assemble`) into a program file. The first way to get
/// an arbitrary program from the command line -- previously `demo` was the
/// only way to get a program file at all (see `docs/gindema-cli.md`'s
/// former "Known limits").
pub fn run_assemble(source: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let text = std::fs::read_to_string(source)?;
    let program = assemble(&text)?;
    let bytes = program.to_bytes();
    std::fs::write(output, &bytes)?;
    println!(
        "wrote {} ({} bytes) from {}",
        output.display(),
        bytes.len(),
        source.display(),
    );
    Ok(())
}

/// Packs `assets` (already-read bytes, in order) into a new archive under a
/// freshly generated key. Kept separate from `run_pack`'s file I/O so the
/// packing logic itself is unit-testable without touching the filesystem,
/// mirroring `build_demo_program` above.
pub fn pack_assets(
    assets: &[Vec<u8>],
    honeypot_count: u32,
    seed: u64,
) -> Result<(ContainerKey, Vec<u8>), ContainerError> {
    let mut builder = ContainerBuilder::new();
    for asset in assets {
        builder.add_asset(asset.clone())?;
    }
    let key = ContainerKey::generate();
    let archive = builder.build(&key, honeypot_count, seed)?;
    Ok((key, archive))
}

/// Extracts every real asset from an already-built archive, in order. Kept
/// separate from `run_unpack`'s file I/O for the same reason as
/// `pack_assets`.
pub fn unpack_all(data: &[u8], key: ContainerKey) -> Result<Vec<Vec<u8>>, ContainerError> {
    let reader = ContainerReader::open(data, key)?;
    (0..reader.asset_count() as u32)
        .map(|order| reader.read_asset(order))
        .collect()
}

/// `pack <output> <input>... [--honeypots N] [--seed S]`: packs one or more
/// files into a new gindema-container archive via `pack_assets`. Generates
/// a fresh key and prints it -- there is no other way to get it back, so it
/// must be saved now to unpack later.
pub fn run_pack(
    output: &Path,
    inputs: &[std::path::PathBuf],
    honeypot_count: u32,
    seed: u64,
) -> Result<(), Box<dyn Error>> {
    let mut assets = Vec::with_capacity(inputs.len());
    for input in inputs {
        assets.push(std::fs::read(input)?);
        println!("  added {}", input.display());
    }
    let (key, archive) = pack_assets(&assets, honeypot_count, seed)?;
    std::fs::write(output, &archive)?;
    println!(
        "wrote {} ({} bytes, {} real asset(s), {honeypot_count} honeypot(s))",
        output.display(),
        archive.len(),
        inputs.len(),
    );
    println!(
        "key (save this -- it's the only way to unpack): {}",
        key.to_base64()
    );
    Ok(())
}

/// `unpack <input> --key <BASE64> <output-dir>`: opens an archive via
/// `unpack_all` and writes every real asset to `output_dir`. The container
/// stores no filenames by design (see `docs/gindema-container.md`'s "Known
/// limits"), so extracted files are named by their 0-based asset order, not
/// an original name.
pub fn run_unpack(input: &Path, key_b64: &str, output_dir: &Path) -> Result<(), Box<dyn Error>> {
    let data = std::fs::read(input)?;
    let key = ContainerKey::from_base64(key_b64)?;
    let assets = unpack_all(&data, key)?;
    std::fs::create_dir_all(output_dir)?;
    for (order, bytes) in assets.iter().enumerate() {
        let path = output_dir.join(format!("asset-{order:04}.bin"));
        std::fs::write(&path, bytes)?;
        println!("  wrote {}", path.display());
    }
    println!(
        "extracted {} asset(s) from {} into {} (filenames aren't stored -- see docs/gindema-container.md)",
        assets.len(),
        input.display(),
        output_dir.display(),
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

    #[test]
    fn flattening_the_demo_program_preserves_its_behavior() {
        let program = build_demo_program(0, 0);
        let flattened = flatten_control_flow(&program, 13).unwrap();
        assert_ne!(flattened.code_bytes(), program.code_bytes());
        let mut vm = Vm::new();
        assert_eq!(vm.run(&flattened).unwrap(), Some(Value::Int(55)));
    }

    #[test]
    fn fluffing_the_demo_program_preserves_its_behavior() {
        let program = build_demo_program(0, 0);
        let fluffed = obfuscate_with_fluff(&program, 14).unwrap();
        assert!(fluffed.code_bit_len() >= program.code_bit_len());
        let mut vm = Vm::new();
        assert_eq!(vm.run(&fluffed).unwrap(), Some(Value::Int(55)));
    }

    #[test]
    fn flattening_then_fluffing_then_remapping_the_demo_program_still_composes() {
        // The CLI's own recommended pipeline order (see run_flatten's doc
        // comment): flatten, then fluff, then remap.
        let program = build_demo_program(1, 15);
        let flattened = flatten_control_flow(&program, 16).unwrap();
        let fluffed = obfuscate_with_fluff(&flattened, 17).unwrap();
        let remapped = remap_opcodes(&fluffed, 18).unwrap();
        assert_eq!(remapped.fluff_seed(), Some(17));

        let mut vm = Vm::new();
        assert_eq!(vm.run(&remapped).unwrap(), Some(Value::Int(55)));
    }

    #[test]
    fn assembling_text_produces_a_program_that_runs() {
        let program = assemble("load_const 6\nload_const 7\nmul\nhalt\n").unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(42)));
    }

    #[test]
    fn pack_then_unpack_round_trips_the_original_assets() {
        let assets = vec![
            b"pretend this is texture data".to_vec(),
            b"pretend this is sound data, a bit longer".to_vec(),
        ];
        let (key, archive) = pack_assets(&assets, 3, 42).unwrap();
        let restored = unpack_all(&archive, key).unwrap();
        assert_eq!(restored, assets);
    }

    #[test]
    fn honeypots_dont_change_which_assets_come_back_or_their_order() {
        let assets = vec![b"a".to_vec(), b"bb".to_vec(), b"ccc".to_vec()];
        let (key, archive) = pack_assets(&assets, 5, 7).unwrap();
        let restored = unpack_all(&archive, key).unwrap();
        assert_eq!(restored, assets);
    }

    #[test]
    fn unpacking_with_the_wrong_key_fails() {
        let assets = vec![b"only asset".to_vec()];
        let (_key, archive) = pack_assets(&assets, 0, 1).unwrap();
        let wrong_key = ContainerKey::generate();
        assert!(unpack_all(&archive, wrong_key).is_err());
    }
}
