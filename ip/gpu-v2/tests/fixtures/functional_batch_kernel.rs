use gpu_v2::system::{functional::Executor, vliw_command::DeviceSubmission};
use std::{hint::black_box, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).unwrap();
    let flags = std::fs::read(&args[2]).unwrap();
    let expected = std::fs::read(&args[3]).unwrap();
    let words: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let expected: Vec<_> = expected
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let base: u32 = args[4].parse().unwrap();
    let count: u32 = args[5].parse().unwrap();
    let captured = args[6] == "captured";
    let repeats: usize = args[7].parse().unwrap();
    let maximum: usize = args.get(9).map_or(64_000_000, |s| s.parse().unwrap());
    for sample in 0..repeats {
        let mut memory = words.clone();
        let mut access = flags.clone();
        let mut executor = Executor::new(maximum);
        #[cfg(batched)]
        {
            use gpu_v2::system::functional::RebinMode;
            executor = executor.with_rebin_mode(match args[8].as_str() {
                "A" => RebinMode::TriangleTiles,
                "B" => RebinMode::TileScan,
                "C" => RebinMode::ParallelSort,
                "D" => RebinMode::LocalBuckets,
                _ => panic!("rebin"),
            });
        }
        let submission = DeviceSubmission::new(base, (count + 1) * 4).unwrap();
        let start = Instant::now();
        let result = if captured {
            executor.execute_captured(black_box(&mut memory), black_box(&mut access), submission)
        } else {
            executor.execute(black_box(&mut memory), submission)
        };
        let seconds = start.elapsed().as_secs_f64();
        let report = result.unwrap();
        black_box(report);
        assert!(memory == expected, "full8MiB mismatch");
        let pool_threads = rayon::current_num_threads();
        #[cfg(batched)]
        let batch = executor.batch_stats().to_json();
        #[cfg(not(batched))]
        let batch = "{}";
        println!("{{\"sample\":{sample},\"seconds\":{seconds:.9},\"pool_threads\":{pool_threads},\"batch\":{batch}}}");
    }
}
