//! Test-only one-triangle topology. No future mesh, producer rows or second clock.
use gpu_v2::frontend::{
    ports::Input,
    sim::{
        runtime::{FrontendOut, Profile, Sequencer},
        timed::{Action, Config},
    },
    source_capture::{Controller, ProducerPermit, Task},
};

pub struct Frontend<'a> {
    pub engine: Sequencer<'a>,
    pending: Option<Task>,
    ordinal: u32,
    context: u64,
    pub admitted: u64,
    pub retries: u64,
}
impl<'a> Frontend<'a> {
    pub fn new(input: &'a Input, config: Config, context: u64) -> Result<Self, String> {
        Ok(Self {
            engine: Sequencer::new(input, config, Profile::ConnectedTriangles)?,
            pending: None,
            ordinal: 0,
            context,
            admitted: 0,
            retries: 0,
        })
    }
    pub fn pending(&self) -> Option<Task> {
        self.pending
    }
    pub fn topology_drained(&self) -> bool {
        self.engine.done() && self.pending.is_none()
    }
    /// Caller captured permit before its unique Connection pump. Existing DMA
    /// advances on wall edges; source/task mutations are separately CE-gated.
    pub fn after_clock(
        &mut self,
        src: &mut Controller,
        permit: ProducerPermit,
        ce: bool,
    ) -> Result<FrontendOut, String> {
        if src.cycles() != self.engine.wall() + 1 {
            return Err("frontend/source clock mismatch".into());
        }
        let epochs = src.slots().each_ref().map(|s| s.epoch);
        let mut port = src.bind_producer_edge(permit)?;
        let out = self.engine.step(ce, self.pending.is_none(), &mut port)?;
        if ce {
            if let Some(task) = self.pending {
                if let Some(ticket) = port.submit_task(task)? {
                    port.seal_after_admission(task.slot, task.epoch, ticket)?;
                    self.pending = None;
                    self.admitted += 1;
                } else {
                    self.retries += 1;
                }
            }
        }
        for record in &out.records {
            if let Action::DrawDone { slot, .. } = record.action {
                if self.pending.is_some() || self.ordinal >= 6 {
                    return Err("topology receipt bound".into());
                }
                self.pending = Some(Task {
                    triangle_id: 7,
                    slot,
                    epoch: epochs[slot],
                    vertices: [0, 1, 2],
                    context: self.context,
                });
                self.ordinal += 1;
            }
        }
        Ok(out)
    }
}
