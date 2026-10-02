#[path = "support/sdram/burst.rs"]
mod burst;

use digital_design_hardware_gowin::sdram_memory_controller::ports::OracleImage;
use gpu_v2::memory::ports::{MemoryPort, Request, BURST_BEATS};

fn pattern(sector: usize, beat: usize) -> u64 {
    0x2468_1357_abcd_0000 ^ ((sector as u64 + 1) << 48) ^ (beat as u64 * 0x1000_0001)
}

#[test]
fn four_real_serial_bursts_hold_ownership_until_write_ack() {
    let mut port =
        burst::Adapter::new(OracleImage::filled::<0xa5>(0, 8192).unwrap(), 10000).unwrap();
    for sector in 0..4 {
        let mut last_write = None;
        let mut acknowledgement = None;
        let request = Request {
            address_bytes: 4096 + sector as u64 * 128,
            write: true,
        };
        let mut accepted = false;
        let mut fed = 0;
        let mut complete = false;
        for wall in 0..2000 {
            // The compute path may be paused. This source and its reservation
            // keep clocking independently; no compute CE enters MemoryPort.
            let _compute_ce = wall % 11 >= 7;
            let response = port
                .cycle(
                    (!accepted).then_some(request),
                    (fed < BURST_BEATS).then(|| pattern(sector, fed)),
                )
                .unwrap();
            assert!(!accepted || !response.accepted);
            accepted |= response.accepted;
            if response.write_accepted {
                assert!(accepted);
                fed += 1;
                if fed == BURST_BEATS {
                    last_write = Some(port.combination.cycle);
                }
            }
            if let Some(success) = response.complete {
                assert!(success && accepted);
                assert_eq!(fed, BURST_BEATS);
                acknowledgement = Some(port.combination.cycle);
                complete = true;
                break;
            }
        }
        assert!(complete && port.idle(), "bounded write did not finish");
        assert!(
            acknowledgement.unwrap() > last_write.unwrap(),
            "last data is not the write ACK"
        );
    }
    for sector in 0..4 {
        let request = Request {
            address_bytes: 4096 + sector as u64 * 128,
            write: false,
        };
        let mut accepted = false;
        let mut got = Vec::new();
        let mut complete = false;
        for _ in 0..2000 {
            let response = port.cycle((!accepted).then_some(request), None).unwrap();
            accepted |= response.accepted;
            if let Some((index, data)) = response.read {
                assert_eq!(index as usize, got.len());
                got.push(data);
            }
            if let Some(success) = response.complete {
                assert!(success);
                complete = true;
                break;
            }
        }
        assert!(complete && accepted && port.idle());
        assert_eq!(got, (0..16).map(|b| pattern(sector, b)).collect::<Vec<_>>());
    }
    let bytes = port.combination.bridge.pins.bytes();
    assert!(bytes[..4096].iter().all(|&b| b == 0xa5));
    assert!(bytes[4096 + 512..].iter().all(|&b| b == 0xa5));
    assert_eq!(port.combination.bridge.write_chains, 0);
    assert_eq!(port.combination.bridge.read_chains, 0);
}

#[test]
fn blocked_descriptor_and_data_must_be_stable_and_first_beat_reserved() {
    let mut port = burst::Adapter::new(OracleImage::filled::<0>(0, 4096).unwrap(), 1000).unwrap();
    let request = Request {
        address_bytes: 128,
        write: true,
    };
    for _ in 0..8 {
        let response = port.cycle(Some(request), None).unwrap();
        assert!(!response.accepted && !response.write_accepted);
    }
    assert!(port
        .cycle(
            Some(Request {
                address_bytes: 256,
                ..request
            }),
            None
        )
        .is_err());
    assert!(!port.cycle(Some(request), Some(123)).unwrap().accepted);
    assert!(port.cycle(Some(request), Some(124)).is_err());
    assert!(!port.idle());
}

#[test]
fn invalid_addresses_are_rejected_before_any_physical_acceptance() {
    let mut port = burst::Adapter::new(OracleImage::filled::<0>(0, 4096).unwrap(), 1000).unwrap();
    for address in [1, 127, u64::MAX, 4096, u64::MAX - 127] {
        assert!(port
            .cycle(
                Some(Request {
                    address_bytes: address,
                    write: false
                }),
                None
            )
            .is_err());
        assert!(port.idle());
        assert_eq!(port.combination.cycle, 0);
    }
}

#[test]
fn port_does_not_allow_a_second_request_before_terminal_ack() {
    let mut port = burst::Adapter::new(OracleImage::filled::<0>(0, 4096).unwrap(), 2000).unwrap();
    let request = Request {
        address_bytes: 128,
        write: false,
    };
    let mut accepted = false;
    for _ in 0..1000 {
        if port.cycle(Some(request), None).unwrap().accepted {
            accepted = true;
            break;
        }
    }
    assert!(accepted);
    assert!(port.cycle(Some(request), None).is_err());
    let mut complete = false;
    for _ in 0..1000 {
        if port.cycle(None, None).unwrap().complete == Some(true) {
            complete = true;
            break;
        }
    }
    assert!(complete && port.idle());
}

#[test]
fn accepted_write_requires_reserved_continuous_source() {
    let mut port = burst::Adapter::new(OracleImage::filled::<0>(0, 4096).unwrap(), 2000).unwrap();
    let request = Request {
        address_bytes: 128,
        write: true,
    };
    let mut accepted = false;
    let mut first_consumed = false;
    for _ in 0..1000 {
        let r = port
            .cycle((!accepted).then_some(request), Some(0x1234_5678))
            .unwrap();
        accepted |= r.accepted;
        if r.write_accepted {
            first_consumed = true;
            assert!(accepted && r.complete.is_none());
            break;
        }
    }
    assert!(first_consumed);
    let mut rejected = false;
    for _ in 0..64 {
        match port.cycle(None, None) {
            Err(e) => {
                assert!(e.contains("source underrun"));
                rejected = true;
                break;
            }
            Ok(r) => assert!(r.complete.is_none()),
        }
    }
    // This is an illegal source/protocol failure, not a simulated recoverable
    // memory error. Do not claim that the rejected partial transfer completed.
    assert!(rejected && !port.idle());
}
