//! Replay actual CPU-recorded inputs and compare the independent geometry oracle.
use gpu_v2::system::{functional::Executor, vliw_command::DeviceSubmission};
use std::path::Path;

pub(super) fn replay(out: &Path, frames: usize) {
    let mut memory = vec![0x55aa; 0x400000];
    let assets = std::fs::read(out.join("assets.bin")).unwrap();
    for (word, data) in memory[0x4e0000 / 2..]
        .iter_mut()
        .zip(assets.as_chunks::<2>().0)
    {
        *word = u16::from_le_bytes(*data);
    }
    let mut cursor = 0;
    for line in std::fs::read_to_string(out.join("cpu-gpu-input.hex"))
        .unwrap()
        .lines()
    {
        if let Some(address) = line.strip_prefix('@') {
            cursor = usize::from_str_radix(address, 16).unwrap() * 4;
        } else {
            let value = u64::from_str_radix(line, 16).unwrap();
            for lane in 0..4 {
                memory[cursor + lane] = (value >> (16 * lane)) as u16;
            }
            cursor += 4;
        }
    }
    let initial = memory.clone();
    let count: u32 = std::fs::read_to_string(out.join("submit-count.txt"))
        .unwrap()
        .parse()
        .unwrap();
    let mut executor = Executor::new(64_000_000);
    for frame in 1..=frames {
        executor
            .execute(
                &mut memory,
                DeviceSubmission::new(0x4c0000, (count + 1) * 4).unwrap(),
            )
            .unwrap();
        for (name, base) in [("gold", 0x440000), ("depth", 0x480000)] {
            let expected: Vec<u16> =
                std::fs::read_to_string(out.join(format!("{name}{frame}.hex")))
                    .unwrap()
                    .lines()
                    .map(|v| u16::from_str_radix(v, 16).unwrap())
                    .collect();
            assert_eq!(expected.len(), 400 * 240);
            let mut actual = String::new();
            for y in 0..240 {
                for x in 0..400 {
                    let offset = ((x / 16) * 16 + y / 16) * 256 + (y % 16) * 16 + x % 16;
                    let got = memory[base / 2 + offset];
                    let want = expected[y * 400 + x];
                    if name == "gold" || want == 65535 {
                        assert_eq!(got, want, "{name} frame{frame} ({x},{y})");
                    } else {
                        assert!(
                            got.abs_diff(want) <= 1,
                            "depth frame{frame} ({x},{y}): {got}/{want}"
                        );
                    }
                    use std::fmt::Write;
                    writeln!(actual, "{got:04x}").unwrap();
                }
            }
            std::fs::write(out.join(format!("functional-{name}{frame}.hex")), actual).unwrap();
        }
        for (address, (&before, &after)) in initial.iter().zip(&memory).enumerate() {
            let byte = address * 2;
            if !(0x440000..0x440000 + 204800).contains(&byte)
                && !(0x480000..0x480000 + 204800).contains(&byte)
            {
                assert_eq!(before, after, "outside writable planes at byte {byte:#x}");
            }
        }
    }
    std::fs::write(out.join("functional-result.txt"), format!("PASS frames={frames}; 96000 RGB565 pixels exact; depth <=1; immutable inputs and allocation guards unchanged; work limit=64000000 per submission\n")).unwrap();
}
