use super::super::ports::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventTick {
    pub cycle: u64,
    pub set: u8,
    pub ack: u8,
    pub boundary: bool,
    pub handler: Option<u8>,
    pub pending: u8,
}
pub fn audit_events(trace: &[EventTick], max_cycles: u64) -> Result<(), String> {
    if trace.len() as u64 > max_cycles {
        return Err("event trace cycle bound".into());
    }
    let mut state = EventState::default();
    let mut last = None;
    for row in trace {
        if row.cycle >= max_cycles || last.is_some_and(|c| row.cycle <= c) {
            return Err("event trace order".into());
        }
        last = Some(row.cycle);
        state.update(row.set, 0);
        let chosen = state.take(row.boundary);
        state.update(row.set, row.ack);
        if chosen != row.handler || state.pending != row.pending {
            return Err("event handler/pending certificate".into());
        }
        if row.handler.is_some_and(|id| id > 7 || id == 5 || id == 6) {
            return Err("masked handler dispatched".into());
        }
    }
    Ok(())
}
