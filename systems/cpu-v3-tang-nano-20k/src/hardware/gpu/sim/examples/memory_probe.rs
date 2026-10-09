use gpu_v2_cmodel::probe::ReadTrace;
use gpu_v2_cmodel::timing::{MemoryTiming, ReadBurst, ReadMemory};

fn main() {
    let seed = std::env::args()
        .nth(1)
        .map(|argument| argument.parse::<u64>().expect("seed must be a u64"))
        .unwrap_or(19);
    let timing = MemoryTiming {
        grant_wait: 2,
        first_beat: 3,
        beat_gap: 1,
        jitter: 2,
    };
    let mut memory = ReadMemory::new(vec![0x11, 0x22, 0x33, 0x44], timing, seed);
    let burst = ReadBurst {
        address: 0,
        beats: 4,
    };
    let mut trace = ReadTrace::new(64);
    let mut request_pending = true;
    let mut received = Vec::new();
    for edge in 1..=64 {
        let ready = edge % 3 != 0;
        let output = trace
            .step(&mut memory, request_pending.then_some(burst), ready)
            .expect("read service violated the bus contract");
        if output.request_accepted {
            request_pending = false;
        }
        if output.response_accepted {
            received.push(output.response.expect("accepted beat is present").data);
        }
        if trace.summary().is_some() {
            break;
        }
    }
    assert_eq!(received, [0x11, 0x22, 0x33, 0x44]);
    let summary = trace
        .summary()
        .expect("read did not finish within 64 edges");
    println!("seed={seed} timing={timing:?} summary={summary:?}");
    print!("{}", trace.render());
}
