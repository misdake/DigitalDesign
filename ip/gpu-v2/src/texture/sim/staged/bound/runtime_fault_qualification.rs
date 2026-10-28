//! Terminal parent errors preserve the independent physical request owner.
use super::{qualification as q, *};
use crate::texture::ports::{Filter, RefillEvent, RefillPort};
use digital_design_hardware_gowin::sdram_memory_controller::{
    combination::idle_inputs,
    ports::{Client, Event, Request, Service},
};
use std::{fmt::Write, path::PathBuf};

#[derive(Debug)]
struct RequestOwner {
    id: u64,
    address: u64,
    bytes: usize,
    started: bool,
    beats: usize,
    complete: bool,
}
struct PhysicalOwner {
    physical: q::physical::Physical,
    owners: Vec<RequestOwner>,
    held: usize,
    trace: String,
    display: Vec<RequestOwner>,
}
impl PhysicalOwner {
    fn finished(&self) -> bool {
        self.owners.iter().all(|r| r.complete) && self.physical.service.idle()
    }
}
impl RefillPort for PhysicalOwner {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        let id = self.physical.submit_read(address, bytes)?;
        self.owners.push(RequestOwner {
            id,
            address,
            bytes,
            started: false,
            beats: 0,
            complete: false,
        });
        writeln!(
            self.trace,
            "submit wall={} id={id} address={address} bytes={bytes}",
            self.physical.cycles
        )
        .unwrap();
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        // Mirror only the service's published read-only descriptor view. This
        // does not clock the MC; service.step below remains the sole owner.
        let presented = self
            .owners
            .iter()
            .find(|r| !r.complete)
            .filter(|r| !r.started);
        let expected = presented.map(|r| {
            let mut wires = idle_inputs();
            wires.display_response_ready = true;
            if let Some(d) = self.display.iter().find(|d| !d.complete) {
                wires.display_request_valid = !d.started;
                wires.display_address = d.address / 2;
            }
            wires.gpu_ro_request_valid = true;
            wires.gpu_ro_address = r.address / 2;
            wires.gpu_ro_line_count_minus_1 = (r.bytes / 32 - 1) as u64;
            let ready = self
                .physical
                .service
                .fabric
                .output(&wires)
                .gpu_ro_request_ready;
            if !ready {
                self.held += 1;
            }
            writeln!(
                self.trace,
                "present wall={} id={} valid=1 ready={ready} address={} bytes={}",
                self.physical.cycles, r.id, r.address, r.bytes
            )
            .unwrap();
            (r.id, ready)
        });
        self.physical.cycles += 1;
        let mut events = vec![];
        for event in self.physical.service.step()? {
            let id = match &event {
                Event::Started { id, .. }
                | Event::ReadBeat { id, .. }
                | Event::Complete { id, .. } => *id,
            };
            let Some(owner) = self.owners.iter_mut().find(|r| r.id == id) else {
                // Real higher-priority Display requests delay RO acceptance.
                if let Some(d) = self.display.iter_mut().find(|d| d.id == id) {
                    match event {
                        Event::Started { .. } => d.started = true,
                        Event::ReadBeat { .. } => d.beats += 1,
                        Event::Complete { .. } => {
                            assert_eq!(d.beats, 4);
                            d.complete = true;
                        }
                    }
                }
                continue;
            };
            assert!(!owner.complete, "event after terminal completion");
            match event {
                Event::Started { .. } => {
                    assert_eq!(expected, Some((id, true)), "actual descriptor acceptance");
                    assert!(!owner.started);
                    owner.started = true;
                    events.push(RefillEvent::Started { id });
                }
                Event::ReadBeat {
                    index, data, last, ..
                } => {
                    assert!(owner.started);
                    assert_eq!(index, owner.beats, "ordered accepted burst");
                    assert_eq!(last, index + 1 == owner.bytes / 8);
                    let offset = (owner.address - 4096) as usize + index * 8;
                    let bytes = &self.physical.service.bytes()[offset..offset + 8];
                    assert_eq!(data.bits(), u64::from_le_bytes(bytes.try_into().unwrap()));
                    owner.beats += 1;
                    events.push(RefillEvent::Beat {
                        id,
                        index,
                        data: data.bits(),
                        last,
                    });
                }
                Event::Complete { .. } => {
                    assert!(owner.started);
                    assert_eq!(owner.beats, owner.bytes / 8);
                    owner.complete = true;
                    events.push(RefillEvent::Complete { id });
                }
            }
            writeln!(
                self.trace,
                "event wall={} owner={owner:?}",
                self.physical.cycles
            )
            .unwrap();
        }
        if let Some((id, false)) = expected {
            assert!(
                self.owners
                    .iter()
                    .any(|r| r.id == id && !r.started && !r.complete),
                "presented descriptor was not dropped"
            );
        }
        Ok(events)
    }
}

#[test]
fn terminal_parent_fault_drains_presented_and_partial_accepted_mc_before_recreation() {
    const LIMIT: u64 = 20_000;
    let directory = std::env::var_os("GPU_ORDERED_PACKET_STORAGE_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ordered-pool")
        })
        .join("parent-fault-drain");
    std::fs::create_dir_all(&directory).unwrap();
    for partial in [false, true] {
        let (slots, bytes) = q::fixture();
        let mut port = PhysicalOwner {
            physical: q::physical::Physical::new(4096, bytes, true, false),
            owners: vec![],
            held: 0,
            trace: String::new(),
            display: vec![],
        };
        let config = super::super::control::Hardware {
            max_cycles: LIMIT,
            ..Default::default()
        };
        let mut runtime = q::runtime(&slots, config);
        let input = q::input(0, 1, [0.25, 0.25], Filter::Trilinear);
        assert!(
            runtime
                .step(&mut port, Some(&input), timed::Control::default())
                .unwrap()
                .accepted
        );
        let mut reached = false;
        for _ in 0..LIMIT {
            let live = port.owners.iter().find(|r| !r.complete);
            if live.is_some_and(|r| {
                if partial {
                    r.started && r.beats > 0 && r.beats < 16
                } else {
                    !r.started
                }
            }) {
                reached = true;
                break;
            }
            let ce = port_cycle_ce(port.physical.cycles);
            runtime
                .step(
                    &mut port,
                    None,
                    timed::Control {
                        ce,
                        result_ready: true,
                    },
                )
                .unwrap();
        }
        assert!(reached, "physical fault point watchdog");
        if !partial {
            // Delay actual RO acceptance with a real high-priority request. The
            // service continues driving the original RO address until ready.
            for i in 0..4 {
                let address = 4096 + i * 32;
                let id = port
                    .physical
                    .service
                    .submit(Client::Display, Request::Read { address, bytes: 32 })
                    .unwrap();
                port.display.push(RequestOwner {
                    id,
                    address,
                    bytes: 32,
                    started: false,
                    beats: 0,
                    complete: false,
                });
            }
        }
        let counts = (port.physical.cycles, port.owners.len());
        let descriptor = runtime.cache.demand_owner_probe();
        assert_eq!((descriptor.0, descriptor.1, descriptor.2), (0, 1, partial));
        if partial {
            assert!((1..16).contains(&descriptor.3));
        }
        let before = runtime.snapshot();
        let mut bad = input.clone();
        bad.quad_id = 16;
        let error = runtime
            .step(&mut port, Some(&bad), timed::Control::default())
            .unwrap_err();
        assert!(error.contains("quad ID exceeds"));
        assert!(runtime.faulted());
        assert_eq!(
            runtime.snapshot(),
            before,
            "rejected parent edge had no data transfers"
        );
        assert_eq!((port.physical.cycles, port.owners.len()), counts);
        let stopped = runtime.snapshot();
        assert!(runtime
            .step(&mut port, None, timed::Control::default())
            .is_err());
        assert_eq!(runtime.snapshot(), stopped);
        assert_eq!(runtime.cache.demand_owner_probe(), descriptor);
        assert_eq!((port.physical.cycles, port.owners.len()), counts);
        writeln!(port.trace, "parent terminal partial={partial} {counts:?}").unwrap();
        let mut drain = 0;
        while !port.finished() {
            assert!(drain < LIMIT, "external accepted-MC drain watchdog");
            drain += 1;
            let _discarded = port.step().unwrap();
            assert_eq!(
                runtime.snapshot(),
                stopped,
                "external drain cannot revive compute"
            );
            assert_eq!(runtime.cache.demand_owner_probe(), descriptor);
            assert_eq!(
                port.owners.len(),
                counts.1,
                "terminal path submitted no new requests"
            );
        }
        assert!(port
            .owners
            .iter()
            .all(|r| r.started && r.beats == 16 && r.complete));
        assert!(drain > 0);
        if !partial {
            assert!(port.held > 0, "presented RO really waited for acceptance");
        }
        let old_owners = port.owners.len();
        // No transport reset, ID cancellation, image reload or runtime retry.
        // Reconstruction occurs only after every original owner reached terminal.
        let mut fresh = q::runtime(&slots, config);
        let mut offered = Some(&input);
        let mut pixels = vec![];
        let mut done = false;
        for _ in 0..LIMIT {
            let step = fresh
                .step(&mut port, offered, timed::Control::default())
                .unwrap();
            if step.accepted {
                offered = None;
            }
            pixels.extend(step.results);
            if offered.is_none() && fresh.idle() && port.finished() {
                done = true;
                break;
            }
        }
        assert!(done, "recreated Runtime watchdog");
        assert_eq!(pixels, q::golden(&input));
        assert!(port.owners.len() > old_owners);
        let name = if partial {
            "partial-accepted"
        } else {
            "presented-not-accepted"
        };
        std::fs::write(directory.join(format!("{name}.trace.txt")), &port.trace).unwrap();
        std::fs::write(directory.join(format!("{name}.json")),format!("{{\"fault_point\":\"{name}\",\"external_drain_edges\":{drain},\"old_requests\":{old_owners},\"presented_wait_edges\":{},\"old_runtime_frozen\":true,\"old_bursts_16_beats_terminal\":true,\"reconstructed_golden_pixels\":{},\"transport_reset\":false}}\n",port.held,pixels.len())).unwrap();
        println!(
            "PARENT_DRAIN {name} old={old_owners} drain={drain} held={} fresh_pixels={}",
            port.held,
            pixels.len()
        );
    }
}
fn port_cycle_ce(wall: u64) -> bool {
    wall % 7 > 1
}
