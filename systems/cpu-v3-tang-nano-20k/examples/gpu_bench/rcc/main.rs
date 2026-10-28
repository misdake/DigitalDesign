//! Static GPU-only batches first; visible previews only after all measurements.
use crate::dsl_rt::*;
mod device_abi;
use device_abi::*;
mod time;
use time::*;
mod boot_resources;
use boot_resources::*;

static PACKET: Buf<u16, 20> = Buf::new([0; 20]);
static RESULT: Buf<u16, 3> = Buf::new([0; 3]);
static SAMPLES: Buf<u16, 124> = Buf::new([0; 124]);

fn byte(value: u16) {
    while dev_recv(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_STATUS) & 1 != 0 {}
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_UART_TX_DATA, value);
}

fn report(kind: u16, case_id: u16, sample: u16, us: u16, ms: u16, flags: u16) {
    let mut packet_flags = flags;
    if us == 65535 {
        packet_flags |= 1;
    }
    PACKET[0u16] = 71;
    PACKET[1u16] = 66;
    PACKET[2u16] = 69;
    PACKET[3u16] = 78;
    PACKET[4u16] = 1;
    PACKET[5u16] = kind;
    PACKET[6u16] = case_id & 255;
    PACKET[7u16] = case_id >> 8;
    PACKET[8u16] = sample & 255;
    PACKET[9u16] = sample >> 8;
    PACKET[10u16] = us & 255;
    PACKET[11u16] = us >> 8;
    PACKET[12u16] = ms & 255;
    PACKET[13u16] = ms >> 8;
    PACKET[14u16] = packet_flags & 255;
    PACKET[15u16] = packet_flags >> 8;
    PACKET[16u16] = BENCH_TOKEN & 255;
    PACKET[17u16] = BENCH_TOKEN >> 8;
    let mut crc: u16 = 65535;
    let mut i: u16 = 0;
    while i < 18 {
        crc ^= PACKET[i] << 8;
        let mut b: u16 = 0;
        while b < 8 {
            if crc & 32768 != 0 {
                crc = (crc << 1) ^ 4129;
            } else {
                crc <<= 1;
            }
            b += 1;
        }
        i += 1;
    }
    PACKET[18u16] = crc & 255;
    PACKET[19u16] = crc >> 8;
    i = 0;
    while i < 20 {
        byte(PACKET[i]);
        i += 1;
    }
}

fn fail(case_id: u16, code: u16) {
    report(128, case_id, code, 0, 0, dev_recv(GPU_DEVICE, GPU_STATUS));
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 63);
    halt(0xde00 | code);
}

fn run(base: u16, count_minus_one: u16, case_id: u16) {
    if dev_recv(GPU_DEVICE, GPU_STATUS) != 0 {
        fail(case_id, 1);
    }
    let start = time_milliseconds();
    time_clear_microseconds();
    dev_send(GPU_DEVICE, GPU_COMMAND_COUNT, count_minus_one);
    dev_send(GPU_DEVICE, GPU_COMMAND_START, base);
    // This hot loop only uses Device4 and the instruction cache. No UART,
    // input upload, framebuffer scanout or next-frame construction is active.
    let mut status = dev_recv(GPU_DEVICE, GPU_STATUS);
    while status & GPU_STATUS_BUSY != 0 {
        let mut spins: u16 = 0;
        while spins < 128 && status & GPU_STATUS_BUSY != 0 {
            status = dev_recv(GPU_DEVICE, GPU_STATUS);
            spins += 1;
        }
        if time_elapsed_ms(start, time_milliseconds()) > 2000 {
            fail(case_id, 2);
        }
    }
    let elapsed = time_microseconds();
    let elapsed_ms = time_elapsed_ms(start, time_milliseconds());
    if status & GPU_STATUS_TRANSPORT_ERROR != 0 {
        fail(case_id, 3);
    }
    RESULT[0u16] = elapsed;
    RESULT[1u16] = elapsed_ms;
    RESULT[2u16] = 0;
    if elapsed == 65535 {
        RESULT[2u16] = 1;
    }
}

fn pause_ms(duration: u16) {
    let start = time_milliseconds();
    while time_elapsed_ms(start, time_milliseconds()) < duration {}
}

pub fn bench_protocol_main() {
    report(1, 7, 3, 12345, 12, 0);
    report(5, 9, 30, 65535, 67, 4);
    halt(0x600d);
}

fn main() {
    dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, 1);
    // Allow the host to attach its UART capture after loading this image.
    pause_ms(8000);
    report(0, BENCH_CASES, BENCH_SAMPLES, 0, 0, 0);
    let mut case_id: u16 = 0;
    while case_id < BENCH_CASES {
        let row = case_id << 2;
        dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, case_id & 31);
        let mut sample: u16 = 0;
        while sample < BENCH_WARMUP + BENCH_SAMPLES {
            run(BENCH_TABLE[row], BENCH_TABLE[row + 1], case_id);
            let setup_us = RESULT[0u16];
            let setup_ms = RESULT[1u16];
            run(BENCH_TABLE[row + 2], BENCH_TABLE[row + 3], case_id);
            if sample >= BENCH_WARMUP {
                let at = (sample - BENCH_WARMUP) << 2;
                SAMPLES[at] = RESULT[0u16];
                SAMPLES[at + 1] = RESULT[1u16];
                SAMPLES[at + 2] = setup_us;
                SAMPLES[at + 3] = setup_ms;
            }
            sample += 1;
        }
        sample = 0;
        while sample < BENCH_SAMPLES {
            let at = sample << 2;
            report(1, case_id, sample, SAMPLES[at], SAMPLES[at + 1], 0);
            report(5, case_id, sample, SAMPLES[at + 2], SAMPLES[at + 3], 0);
            sample += 1;
        }
        case_id += 1;
    }
    report(3, BENCH_CASES, BENCH_SAMPLES, 0, 0, 0);
    // Only now enable scanout reads. No displayed case is a headless sample.
    dev_send(DISPLAY_DEVICE, DISPLAY_COLOR_FORMAT, DISPLAY_LINEAR_RGB565);
    let pixel_base = BENCH_PREVIEW_SECTOR << 6;
    dev_send(DISPLAY_DEVICE, DISPLAY_STAGE_FRAMEBUFFER_LOW, pixel_base);
    dev_send(
        DISPLAY_DEVICE,
        DISPLAY_STAGE_FRAMEBUFFER_HIGH,
        BENCH_PREVIEW_SECTOR >> 10,
    );
    dev_send(DISPLAY_DEVICE, DISPLAY_SWAP_COMMAND, DISPLAY_NEXT_SWAP);
    pause_ms(50);
    while true {
        case_id = 0;
        while case_id < BENCH_CASES {
            let row = case_id << 2;
            run(BENCH_TABLE[row], BENCH_TABLE[row + 1], case_id);
            run(BENCH_TABLE[row + 2], BENCH_TABLE[row + 3], case_id);
            report(2, case_id, 0, RESULT[0u16], RESULT[1u16], 4);
            dev_send(SYSTEM_CONTROL_DEVICE, SYSCTL_LED, case_id & 31);
            pause_ms(1200);
            case_id += 1;
        }
    }
}
