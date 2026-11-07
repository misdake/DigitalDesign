//! Independent full-size code/data windows through the fitted CB06 entry.
use cpu_v3::{
    clean_data_line, halt, jump_register, load, load_immediate16, nop, store, wait_data_cache,
    RunOutcome,
};
use cpu_v3_tang_nano_20k::{
    boot::{pack_single_application, SINGLE_BOOT_CODE_WORD, SINGLE_BOOT_STATIC_WORDS},
    system_emu::CpuV3SystemEmu,
    system_sim::{CpuV3SystemSim, VBlankMode},
};

const DATA_EDGES: [u16; 8] = [0, 0x3fff, 0x4000, 0x7fff, 0x8000, 0xbfff, 0xc000, 0xffff];
const SEED: u16 = 0xd00d;
const DATA_SALT: u16 = 0x600d;
const EXPECTED_HALT: u16 = 0xffff ^ DATA_SALT;

fn fixture() -> (Vec<u16>, Vec<u8>) {
    let mut entry = Vec::new();
    for address in DATA_EDGES {
        entry.extend(load_immediate16(1, address));
        entry.extend(load_immediate16(0, address ^ DATA_SALT));
        entry.push(store(0, 1, 0));
        entry.push(clean_data_line(1));
    }
    entry.push(wait_data_cache());
    entry.extend(load_immediate16(2, 0x8000));
    entry.push(jump_register(2));

    let mut words = vec![nop(); 1 << 16];
    words[..entry.len()].copy_from_slice(&entry);
    // Fetch from both halves and the very last word, without a 64K-word walk.
    let mut middle = load_immediate16(2, 0xfffc).to_vec();
    middle.push(jump_register(2));
    words[0x8000..0x8000 + middle.len()].copy_from_slice(&middle);
    let mut tail = load_immediate16(1, 0xffff).to_vec();
    tail.push(load(0, 1, 0));
    tail.push(halt());
    assert_eq!(tail.len(), 4);
    words[0xfffc..].copy_from_slice(&tail);
    let bytes: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let package = pack_single_application(&bytes, SINGLE_BOOT_STATIC_WORDS).unwrap();
    (words, package)
}

fn check_memory(memory: &[u16], code: &[u16]) {
    assert_eq!(
        &memory[SINGLE_BOOT_CODE_WORD..SINGLE_BOOT_CODE_WORD + code.len()],
        code
    );
    for (address, &word) in memory[..1 << 16].iter().enumerate() {
        let expected = if DATA_EDGES.contains(&(address as u16)) {
            address as u16 ^ DATA_SALT
        } else if address < SINGLE_BOOT_STATIC_WORDS {
            0
        } else {
            SEED
        };
        assert_eq!(word, expected, "physical data word {address:#06x}");
    }
    assert_eq!(memory[1 << 16], SEED);
    assert_eq!(memory[SINGLE_BOOT_CODE_WORD - 1], SEED);
    assert_eq!(memory[SINGLE_BOOT_CODE_WORD + code.len()], SEED);
}

#[test]
fn full_code_and_data_windows_are_independent_in_instruction_sim() {
    let (code, package) = fixture();
    let mut machine =
        CpuV3SystemSim::from_single_flash(&package, SEED, VBlankMode::Manual).unwrap();
    let outcome = machine.run(200).unwrap();
    assert!(
        matches!(
            outcome,
            RunOutcome::Halted {
                signal: EXPECTED_HALT,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert_eq!(machine.cpu().code_segment(), 7);
    assert_eq!(machine.cpu().data_segments(), [0, 1, 2, 3]);
    check_memory(machine.cpu().physical_memory_slice(), &code);
}

#[test]
fn full_code_and_data_windows_are_independent_in_native_emu() {
    let (code, package) = fixture();
    let mut machine =
        CpuV3SystemEmu::from_single_flash(&package, SEED, VBlankMode::Manual).unwrap();
    let mut halted = false;
    for _ in 0..20_000 {
        let output = machine.step().unwrap();
        assert!(!output.fault);
        assert_eq!(output.code_segment, 7);
        if output.halted {
            assert_eq!(output.halt_signal, u64::from(EXPECTED_HALT));
            halted = true;
            break;
        }
    }
    assert!(halted, "full-window program exceeded its cycle bound");
    assert_eq!(machine.core().data_segments(), [0, 1, 2, 3]);
    check_memory(&machine.physical_memory(), &code);
}
