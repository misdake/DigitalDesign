//! Direct cycle projection of the current SDRAM Service, without a timing shim.
use digital_design_hardware_gowin::sdram_memory_controller::ports::*;
use gpu_v2::texture::ports::{RefillEvent, RefillPort};
pub struct Adapter<S> {
    pub service: S,
    pub requests: Vec<(u64, u64, usize)>,
    pub events: Vec<Event>,
}
impl<S> Adapter<S> {
    pub fn new(service: S) -> Self {
        Self {
            service,
            requests: vec![],
            events: vec![],
        }
    }
}
impl<S: Service> RefillPort for Adapter<S> {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
        let id = self
            .service
            .submit(Client::GpuReadOnly, Request::Read { address, bytes })?;
        self.requests.push((id, address, bytes));
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
        let events = self.service.step()?;
        let result = events
            .iter()
            .map(|e| match e {
                Event::Started { id, .. } => RefillEvent::Started { id: *id },
                Event::ReadBeat {
                    id,
                    index,
                    data,
                    last,
                    ..
                } => RefillEvent::Beat {
                    id: *id,
                    index: *index,
                    data: data.bits(),
                    last: *last,
                },
                Event::Complete { id, .. } => RefillEvent::Complete { id: *id },
            })
            .collect();
        self.events.extend(events);
        Ok(result)
    }
}
