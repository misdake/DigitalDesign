//! Fixed pending-bit event identity. Handlers are selected only at an idle or
//! WAIT boundary; acknowledgement is separate from entry and set wins a race.

pub const EVENT_COMMAND: u8 = 4;
pub const EVENT_OUTPUT_CREDIT: u8 = 5;
pub const EVENT_CACHE_DONE: u8 = 6;
pub const EVENT_FAULT: u8 = 7;

pub const HANDLER_PC: [u8; 8] = [16, 17, 18, 19, 20, 21, 22, 23];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Events {
    pending: u8,
    mask: u8,
    next: u8,
}

impl Events {
    pub fn pending(&self) -> u8 {
        self.pending
    }

    pub fn select(&self) -> Option<u8> {
        let eligible = self.pending & !self.mask;
        (0..8)
            .map(|offset| (self.next + offset) & 7)
            .find(|&id| eligible & (1 << id) != 0)
    }

    pub fn dispatched(&mut self, id: u8) {
        assert!(id < 8);
        self.next = (id + 1) & 7;
    }

    pub fn set_mask(&mut self, mask: u8) {
        self.mask = mask;
    }

    pub fn edge(&mut self, acknowledge: u8, set: u8) {
        self.pending = (self.pending & !acknowledge) | set;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_identity_and_same_edge_set_wins() {
        let mut events = Events::default();
        events.edge(0, (1 << 0) | (1 << EVENT_COMMAND));
        assert_eq!(events.select(), Some(0));
        events.edge(1 << 0, 1 << 0);
        assert_eq!(events.select(), Some(0));
        events.dispatched(0);
        assert_eq!(events.select(), Some(EVENT_COMMAND));
        events.edge(1 << 0, 0);
        assert_eq!(events.select(), Some(EVENT_COMMAND));
        events.set_mask(1 << EVENT_COMMAND);
        assert_eq!(events.select(), None);
    }
}
