//! Software-only prepared-lease gate. No GPU SEND, Display swap or cache policy.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod gpu_capture_abi;
use gpu_capture_abi::*;
mod gpu_pipeline;
use gpu_pipeline::*;
mod boot_resources;
use boot_resources::*;

// initialized, candidate, stable, candidate_ms. Buttons are normalized high.
static PROFILER_BUTTONS: Buf<u16, 4> = Buf::new([0; 4]);
// attempt32, sequence32, decoded_bytes32, chunks32, dump_ms32, last_ms.
static PROFILER_COUNTERS: Buf<u16, 11> = Buf::new([0; 11]);
// Frame, range and canonical stream CRC32, before final complement.
static PROFILER_CRC: Buf<u16, 6> = Buf::new([0; 6]);
static PROFILER_INPUT: Buf<u16, 256> = Buf::new([0; 256]);
static PROFILER_PAYLOAD: Buf<u16, 280> = Buf::new([0; 280]);
static PROFILER_FLAGS: Buf<u16, 2> = Buf::new([0; 2]);


fn profiler_now() -> u16 {
    dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_MILLISECONDS)
}
fn profiler_buttons_reset() {
    let raw = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS) & 3;
    let mut state = PROFILER_BUTTONS.as_array();
    state[0u16] = 1;
    state[1u16] = raw;
    state[2u16] = raw;
    state[3u16] = profiler_now();
}
fn profiler_button_edges() -> u16 {
    if PROFILER_BUTTONS.as_array()[0u16] == 0 {
        profiler_buttons_reset();
        return 0;
    }
    let raw = dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_BUTTONS) & 3;
    let now = profiler_now();
    let mut state = PROFILER_BUTTONS.as_array();
    if raw != state[1u16] {
        state[1u16] = raw;
        state[3u16] = now;
    }
    if raw != state[2u16] && now - state[3u16] >= 20 {
        let edges = raw & !state[2u16];
        state[2u16] = raw;
        return edges;
    }
    0
}
fn profiler_add(at: u16, value: u16) {
    let mut counters = PROFILER_COUNTERS.as_array();
    let old = counters[at];
    counters[at] += value;
    if counters[at] < old {
        counters[at + 1] += 1;
    }
}
fn profiler_crc_init(at: u16) {
    PROFILER_CRC.as_array()[at] = 65535;
    PROFILER_CRC.as_array()[at + 1] = 65535;
}
fn profiler_crc_byte(at: u16, value: u16) {
    let mut crc = PROFILER_CRC.as_array();
    let mut low = crc[at] ^ (value & 255);
    let mut high = crc[at + 1];
    let mut bit: u16 = 0;
    while bit < 8 {
        let lsb = low & 1;
        low = (low >> 1) | (high << 15);
        high >>= 1;
        if lsb != 0 {
            low ^= 0x8320;
            high ^= 0xedb8;
        }
        bit += 1;
    }
    crc[at] = low;
    crc[at + 1] = high;
}
fn profiler_crc_word(at: u16, value: u16) {
    profiler_crc_byte(at, value);
    profiler_crc_byte(at, value >> 8);
}
fn profiler_tx(byte: u16) {
    if PROFILER_FLAGS.as_array()[0u16] != 0 {
        return;
    }
    let mut polls: u16 = 0;
    while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {
        polls += 1;
        if polls == 65535 {
            PROFILER_FLAGS.as_array()[0u16] = 1;
            return;
        }
    }
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, byte & 255);
}
fn profiler_tx_word(word: u16) {
    profiler_tx(word);
    profiler_tx(word >> 8);
}
fn profiler_notice(code: u16) {
    if GPU_PROFILER_AUTOMATIC_TEST != 0 {
        return;
    }
    // Console telemetry only, outside GPS1 BEGIN..END. It never changes the
    // prepared lease, Display, command bytes or transport record ABI.
    let mut text: Buf<u16, 11> = Buf::new([10, 71, 80, 85, 80, 82, 79, 70, 58, code, 10]);
    let mut i: u16 = 0;
    while i < 11 {
        profiler_tx(text[i]);
        i += 1;
    }
}
fn profiler_frame_word(word: u16) {
    profiler_crc_word(0, word);
    profiler_tx_word(word);
}
fn profiler_emit(descriptor: Array<u16>, kind: u16, words: u16) {
    profiler_crc_init(0);
    profiler_frame_word(0x5047); // GPS1, little-endian.
    profiler_frame_word(0x3153);
    profiler_frame_word(1 | (kind << 8));
    profiler_frame_word(0);
    profiler_frame_word(descriptor[GPU_CAPTURE_H_TOKEN]);
    profiler_frame_word(descriptor[GPU_CAPTURE_H_TOKEN + 1]);
    let counters = PROFILER_COUNTERS.as_array();
    profiler_frame_word(counters[0u16]);
    profiler_frame_word(counters[1u16]);
    profiler_frame_word(counters[2u16]);
    profiler_frame_word(counters[3u16]);
    profiler_frame_word(words * 2);
    profiler_frame_word(0);
    let mut i: u16 = 0;
    while i < words {
        profiler_frame_word(PROFILER_PAYLOAD.as_array()[i]);
        i += 1;
    }
    profiler_tx_word(!PROFILER_CRC.as_array()[0u16]);
    profiler_tx_word(!PROFILER_CRC.as_array()[1u16]);
    profiler_add(2, 1);
}
fn profiler_elapsed() {
    let now = profiler_now();
    let last = PROFILER_COUNTERS.as_array()[10u16];
    profiler_add(8, now - last);
    PROFILER_COUNTERS.as_array()[10u16] = now;
}
fn profiler_compress(words: u16) -> u16 {
    let input = PROFILER_INPUT.as_array();
    let mut output = PROFILER_PAYLOAD.as_array();
    let mut at: u16 = 0;
    let mut out: u16 = 6;
    while at < words {
        let mut run: u16 = 1;
        while at + run < words && input[at + run] == input[at] {
            run += 1;
        }
        if run >= 3 {
            output[out] = 0x8000 | (run - 1);
            output[out + 1] = input[at];
            out += 2;
            at += run;
        } else {
            let begin = at;
            at += run;
            while at < words {
                if at + 2 < words && input[at] == input[at + 1] && input[at] == input[at + 2] {
                    break;
                }
                at += 1;
            }
            output[out] = at - begin - 1;
            out += 1;
            let mut i = begin;
            while i < at {
                output[out] = input[i];
                out += 1;
                i += 1;
            }
        }
    }
    if out - 6 < words {
        output[1u16] = 1;
        return out;
    }
    output[1u16] = 0;
    let mut i: u16 = 0;
    while i < words {
        output[i + 6] = input[i];
        i += 1;
    }
    words + 6
}
fn profiler_dump(descriptor: Array<u16>) -> bool {
    profiler_add(0, 1);
    if PROFILER_COUNTERS.as_array()[0u16] == 0 && PROFILER_COUNTERS.as_array()[1u16] == 0 {
        return false;
    }
    let mut i: u16 = 2;
    while i < 10 {
        PROFILER_COUNTERS.as_array()[i] = 0;
        i += 1;
    }
    PROFILER_COUNTERS.as_array()[10u16] = profiler_now();
    profiler_crc_init(4);
    i = 0;
    while i < GPU_CAPTURE_WORDS {
        let word = descriptor[i];
        PROFILER_PAYLOAD.as_array()[i] = word;
        profiler_crc_word(4, word);
        i += 1;
    }
    profiler_emit(descriptor, 1, GPU_CAPTURE_WORDS);
    let ranges = descriptor[GPU_CAPTURE_H_RANGE_COUNT];
    let mut region: u16 = 0;
    while region < ranges {
        PROFILER_PAYLOAD.as_array()[0u16] = region;
        profiler_crc_word(4, region);
        i = 0;
        while i < GPU_CAPTURE_RANGE_WORDS {
            let word = gpu_capture_range_word(region, i);
            PROFILER_PAYLOAD.as_array()[i + 1] = word;
            profiler_crc_word(4, word);
            i += 1;
        }
        if gpu_capture_error() != GPU_CAPTURE_OK {
            return false;
        }
        profiler_emit(descriptor, 2, GPU_CAPTURE_RANGE_WORDS + 1);
        region += 1;
    }
    region = 0;
    while region < ranges && PROFILER_FLAGS.as_array()[0u16] == 0 {
        let length_low = gpu_capture_range_word(region, GPU_CAPTURE_R_BYTES);
        let length_high = gpu_capture_range_word(region, GPU_CAPTURE_R_BYTES + 1);
        let word_low = (length_low >> 1) | (length_high << 15);
        let word_high = length_high >> 1;
        let mut low: u16 = 0;
        let mut high: u16 = 0;
        profiler_crc_init(2);
        while high < word_high || (high == word_high && low < word_low) {
            let mut words: u16 = 256;
            if high == word_high && word_low - low < words {
                words = word_low - low;
            }
            i = 0;
            while i < words {
                let offset = low + i;
                let mut offset_high = high;
                if offset < low {
                    offset_high += 1;
                }
                let word = gpu_capture_read_word(region, offset, offset_high);
                PROFILER_INPUT.as_array()[i] = word;
                profiler_crc_word(2, word);
                profiler_crc_word(4, word);
                i += 1;
            }
            if gpu_capture_error() != GPU_CAPTURE_OK {
                return false;
            }
            let mut payload = PROFILER_PAYLOAD.as_array();
            payload[0u16] = region;
            payload[2u16] = low << 1;
            payload[3u16] = (high << 1) | (low >> 15);
            payload[4u16] = words * 2;
            payload[5u16] = 0;
            let encoded = profiler_compress(words);
            profiler_emit(descriptor, 3, encoded);
            profiler_add(4, words * 2);
            profiler_add(6, 1);
            let next = low + words;
            if next < low {
                high += 1;
            }
            low = next;
            profiler_elapsed();
        }
        let mut payload = PROFILER_PAYLOAD.as_array();
        payload[0u16] = region;
        payload[1u16] = 0;
        payload[2u16] = length_low;
        payload[3u16] = length_high;
        payload[4u16] = !PROFILER_CRC.as_array()[2u16];
        payload[5u16] = !PROFILER_CRC.as_array()[3u16];
        profiler_emit(descriptor, 4, 6);
        region += 1;
    }
    if PROFILER_FLAGS.as_array()[0u16] != 0 || gpu_capture_error() != GPU_CAPTURE_OK {
        return false;
    }
    profiler_elapsed();
    let counters = PROFILER_COUNTERS.as_array();
    let mut payload = PROFILER_PAYLOAD.as_array();
    payload[0u16] = ranges;
    payload[1u16] = 0;
    payload[2u16] = counters[4u16];
    payload[3u16] = counters[5u16];
    payload[4u16] = counters[6u16];
    payload[5u16] = counters[7u16];
    payload[6u16] = !PROFILER_CRC.as_array()[4u16];
    payload[7u16] = !PROFILER_CRC.as_array()[5u16];
    payload[8u16] = counters[8u16];
    payload[9u16] = counters[9u16];
    profiler_emit(descriptor, 5, 10);
    PROFILER_FLAGS.as_array()[0u16] == 0
}
pub fn profiler_gate(descriptor: Array<u16>) -> u16 {
    if PROFILER_FLAGS.as_array()[0u16] != 0 || gpu_capture_error() != GPU_CAPTURE_OK {
        return GPU_CAPTURE_FATAL;
    }
    if GPU_PROFILER_AUTOMATIC_TEST == 0 && PROFILER_BUTTONS.as_array()[0u16] == 0 {
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 0);
        profiler_notice(82); // R: first prepared gate is ready for user input.
        if PROFILER_FLAGS.as_array()[0u16] != 0 {
            return GPU_CAPTURE_FATAL;
        }
    }
    let automatic = GPU_PROFILER_AUTOMATIC_TEST != 0 && PROFILER_FLAGS.as_array()[1u16] < 3;
    if automatic {
        PROFILER_FLAGS.as_array()[1u16] += 1;
        if PROFILER_FLAGS.as_array()[1u16] == 1 {
            return GPU_CAPTURE_CONTINUE;
        }
    } else if profiler_button_edges() & 2 == 0 {
        return GPU_CAPTURE_CONTINUE;
    }
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 2);
    profiler_notice(80); // P: producer paused before submission.
    if automatic {
        // Explicit diagnostic stimulus, not a physical-button qualification.
        // First two dumps share one lease; the next lease exposes its result
        // in old Front and depth. Finally remain paused for host verification.
        if PROFILER_FLAGS.as_array()[1u16] == 2 {
            // Open the TX-only receiver after SRAM programming has completed;
            // no bytes from a previous FPGA image can enter this capture.
            let begin = profiler_now();
            while profiler_now() - begin < 20000 {
                if gpu_capture_error() != GPU_CAPTURE_OK {
                    return GPU_CAPTURE_FATAL;
                }
            }
        }
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 4);
        if !profiler_dump(descriptor) {
            PROFILER_FLAGS.as_array()[0u16] = 1;
            return GPU_CAPTURE_FATAL;
        }
        if PROFILER_FLAGS.as_array()[1u16] == 2 {
            if !profiler_dump(descriptor) {
                PROFILER_FLAGS.as_array()[0u16] = 1;
                return GPU_CAPTURE_FATAL;
            }
            dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 0);
            return GPU_CAPTURE_RESUMED;
        }
        profiler_buttons_reset();
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 2);
    }
    // CPU owns the prepared lease for this entire call. Display retains Front.
    while PROFILER_FLAGS.as_array()[0u16] == 0 {
        let edges = profiler_button_edges();
        if gpu_capture_error() != GPU_CAPTURE_OK {
            return GPU_CAPTURE_FATAL;
        }
        if edges & 2 != 0 {
            dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 0);
            profiler_notice(82); // R: resume the same prepared CB once.
            if PROFILER_FLAGS.as_array()[0u16] != 0 {
                return GPU_CAPTURE_FATAL;
            }
            return GPU_CAPTURE_RESUMED;
        }
        if edges & 1 != 0 {
            dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 4);
            profiler_notice(68); // D: dump starts; no GPU submission.
            if !profiler_dump(descriptor) {
                PROFILER_FLAGS.as_array()[0u16] = 1;
                return GPU_CAPTURE_FATAL;
            }
            // Completing TX is not host validation. Remain paused; a held or
            // simultaneous resume button must not start the frozen submission.
            profiler_buttons_reset();
            dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 2);
            profiler_notice(80); // P: complete transfer, still paused.
        }
    }
    GPU_CAPTURE_FATAL
}
