//! Actual serial cycle MC with separately dispatched background client traffic.
use digital_design_hardware_gowin::sdram_memory_controller::{emu::service, ports::*};
use gpu_v2::texture::ports::{RefillEvent, RefillPort};
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
        // SAFETY: immutable external stimulus, not a numerical intermediate.
        let image = unsafe {
            OracleImage::from_host(base, bytes, "bounded texture physical MC fixture").unwrap()
        };
        let mut service = service::Memory::new(image, Default::default()).unwrap();
        let mut init_cycles = 0;
        if initialized {
            while !service.combination.bridge.output(false).initialized {
                assert!(init_cycles < 100000, "MC initialization watchdog");
                assert!(service.step().unwrap().is_empty());
                init_cycles += 1;
            }
        }
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
