//! Test-only import adaptation of tests/support/sdram/physical_texture.rs.
//! Actual serial cycle MC with separately dispatched background client traffic.
use crate::texture::ports::{RefillEvent, RefillPort};
use digital_design_hardware_gowin::sdram_memory_controller::{emu::service, ports::*};
pub struct Physical {
    pub service: service::Memory,
    pub cycles: u64,
    pub init_cycles: u64,
    pub requests: usize,
    pub loaded: bool,
    base: u64,
    background: [Option<u64>; 3],
}
impl Physical {
    pub fn new(base: u64, bytes: Vec<u8>, initialized: bool, loaded: bool) -> Self {
        // Safe compile-time palette words are external texture stimulus. Fill
        // the image by real MC writes, without relaxing the IP unsafe prohibition.
        let image = OracleImage::filled::<0>(base, bytes.len()).unwrap();
        let mut service = service::Memory::new(
            image,
            service::Config {
                init_cycles: 64,
                max_cycles: 100_000,
                ..Default::default()
            },
        )
        .unwrap();
        let mut init_cycles = 0;
        if initialized {
            while !service.combination.bridge.output(false).initialized {
                assert!(init_cycles < 20_000, "MC initialization watchdog");
                assert!(service.step().unwrap().is_empty());
                init_cycles += 1;
            }
        }
        let mut setup_cycles = 0;
        for (tile, data) in bytes.as_chunks::<128>().0.iter().enumerate() {
            let words = data
                .as_chunks::<8>()
                .0
                .iter()
                .map(|b| {
                    let p = u16::from_le_bytes([b[0], b[1]]);
                    assert!(b
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .all(|v| u16::from_le_bytes([v[0], v[1]]) == p));
                    match p {
                        0xf800 => OracleWord::constant::<0xf800f800f800f800>(),
                        0x07e0 => OracleWord::constant::<0x07e007e007e007e0>(),
                        0x001f => OracleWord::constant::<0x001f001f001f001f>(),
                        0xffff => OracleWord::constant::<0xffffffffffffffff>(),
                        0xffe0 => OracleWord::constant::<0xffe0ffe0ffe0ffe0>(),
                        0xf81f => OracleWord::constant::<0xf81ff81ff81ff81f>(),
                        0x07ff => OracleWord::constant::<0x07ff07ff07ff07ff>(),
                        0 => OracleWord::constant::<0>(),
                        _ => panic!("fixture palette word"),
                    }
                })
                .collect::<Vec<_>>();
            let id = service
                .submit(
                    Client::FramebufferWrite,
                    Request::Write {
                        address: base + tile as u64 * 128,
                        data: words,
                        enables: vec![255; 16],
                    },
                )
                .unwrap();
            loop {
                assert!(setup_cycles < 20_000, "MC image write watchdog");
                setup_cycles += 1;
                if service
                    .step()
                    .unwrap()
                    .iter()
                    .any(|e| matches!(e,Event::Complete {id:done,..} if *done==id))
                {
                    break;
                }
            }
        }
        assert_eq!(service.bytes(), bytes, "complete physical texture image");
        Self {
            service,
            cycles: 0,
            init_cycles,
            requests: 0,
            loaded,
            base,
            background: [None; 3],
        }
    }
}
impl RefillPort for Physical {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        if bytes != 128 {
            return Err("RAW565 fixture requires a single 128B refill".into());
        }
        self.requests += 1;
        self.service
            .submit(Client::GpuReadOnly, Request::Read { address, bytes })
    }
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        self.cycles += 1;
        if self.loaded {
            for (i, (client, period)) in [
                (Client::Display, 128),
                (Client::Instruction, 256),
                (Client::Data, 512),
            ]
            .into_iter()
            .enumerate()
            {
                if self.background[i].is_none()
                    && (self.cycles == 1 || self.cycles.is_multiple_of(period))
                {
                    self.background[i] = Some(self.service.submit(
                        client,
                        Request::Read {
                            address: self.base + 32 * i as u64,
                            bytes: 32,
                        },
                    )?);
                }
            }
        }
        let mut result = vec![];
        for e in self.service.step()? {
            let id = match &e {
                Event::Started { id, .. }
                | Event::ReadBeat { id, .. }
                | Event::Complete { id, .. } => *id,
            };
            if self.background.contains(&Some(id)) {
                if matches!(e, Event::Complete { .. }) {
                    for active in &mut self.background {
                        if *active == Some(id) {
                            *active = None;
                        }
                    }
                }
                continue;
            }
            result.push(match e {
                Event::Started { id, .. } => RefillEvent::Started { id },
                Event::ReadBeat {
                    id,
                    index,
                    data,
                    last,
                    ..
                } => RefillEvent::Beat {
                    id,
                    index,
                    data: data.bits(),
                    last,
                },
                Event::Complete { id, .. } => RefillEvent::Complete { id },
            });
        }
        Ok(result)
    }
}
