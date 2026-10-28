//! Bounded producer gate: sample public ranges, resume the middle submission.
use crate::dsl_rt::*;
mod gpu_pipeline;
use gpu_pipeline::*;
mod gpu_capture_abi;
use gpu_capture_abi::*;

static CAPTURE_FULL_GATES: Buf<u16, 1> = Buf::new([0; 1]);
pub fn profiler_gate(d: Array<u16>) -> u16 {
    let mut i: u16 = 0;
    while i < GPU_CAPTURE_WORDS {
        signal(2, d[i]);
        i += 1;
    }
    let mut r: u16 = 0;
    while r < d[GPU_CAPTURE_H_RANGE_COUNT] {
        i = 0;
        while i < GPU_CAPTURE_RANGE_WORDS {
            signal(3, gpu_capture_range_word(r, i));
            i += 1;
        }
        signal(4, gpu_capture_read_word(r, 0, 0));
        let low = gpu_capture_range_word(r, GPU_CAPTURE_R_BYTES);
        let high = gpu_capture_range_word(r, GPU_CAPTURE_R_BYTES + 1);
        let mut last = (low >> 1) | (high << 15);
        let mut upper = high >> 1;
        if last == 0 {
            last = 65535;
            upper -= 1;
        } else {
            last -= 1;
        }
        signal(5, gpu_capture_read_word(r, last, upper));
        r += 1;
    }
    signal(6, gpu_capture_error());
    CAPTURE_FULL_GATES[0u16] += 1;
    if CAPTURE_FULL_GATES[0u16] == 2 {
        1
    } else {
        0
    }
}
