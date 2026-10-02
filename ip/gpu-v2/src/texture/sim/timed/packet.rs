//! One-write/one-read 64-row 72-bit pool with two owned synchronous heads.
//! References and serials check ownership; payload lives only in banks/heads.
use super::Error;
use std::collections::VecDeque;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    pub quad: u8,
    pub lane: u8,
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pending() -> Pool {
        let mut p = Pool::default();
        let mut e = vec![];
        p.reserve(Owner { quad: 0, lane: 0 }, &mut e).unwrap();
        let w = p.write(0, 1 << 28, &mut e).unwrap();
        p.advance(0, 0, Some(w), &mut e).unwrap();
        p.advance(1, 0, None, &mut e).unwrap();
        p.advance(2, 1, None, &mut e).unwrap();
        assert!(p.pending.is_some());
        p
    }
    #[test]
    fn equal_payload_wrong_return_owner_is_rejected_at_capture() {
        let mut p = pending();
        p.pending.as_mut().unwrap().0.serial += 1;
        let err = p.advance(3, 1, None, &mut vec![]).unwrap_err();
        assert_eq!(err.0, "pool synchronous return reservation/owner");
    }
    #[test]
    fn bank_data_replaced_before_return_is_rejected_at_capture() {
        // Keep the reservation and pending return intact; alter the actual
        // backing row so this exercises the data check, not trace replay.
        let mut p = pending();
        let row = usize::from(p.pending.as_ref().unwrap().0.row);
        p.banks[1][row] = Some(p.banks[1][row].unwrap() ^ 1);
        let err = p.advance(3, 1, None, &mut vec![]).unwrap_err();
        assert_eq!(err.0, "pool returned data replaced before capture");
    }
    #[test]
    fn capture_edge_cannot_be_used_as_consumer_bypass() {
        let mut p = pending();
        p.advance(3, 1, None, &mut vec![]).unwrap();
        let err = p.consume(3, p.front().unwrap(), &mut vec![]).unwrap_err();
        assert_eq!(err.0, "pool consume owner/order/value");
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Reserve {
        serial: u64,
        row: u8,
        owner: Owner,
    },
    Write {
        serial: u64,
        row: u8,
        payload: i128,
    },
    Transfer {
        serial: u64,
        row: u8,
    },
    Read {
        serial: u64,
        row: u8,
        head: usize,
    },
    Capture {
        serial: u64,
        row: u8,
        head: usize,
        payload: i128,
    },
    Consume {
        serial: u64,
        head: usize,
        payload: i128,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub producer: usize,
    pub unwritten: usize,
    pub groups: usize,
    pub rows: usize,
    pub heads: usize,
    pub pending: bool,
}
#[derive(Clone, Copy)]
struct Ref {
    serial: u64,
    row: u8,
    owner: Owner,
    written: Option<u64>,
}
pub(super) struct Pool {
    banks: [[Option<u64>; 64]; 2],
    owners: [Option<u64>; 64],
    producer: VecDeque<Ref>,
    groups: VecDeque<Ref>,
    pending: Option<(Ref, i128, usize, u64)>,
    heads: [Option<(Ref, i128)>; 2],
    read_head: usize,
    pop_head: usize,
    allocate: u64,
    reclaim: u64,
    consumed: u64,
}
impl Default for Pool {
    fn default() -> Self {
        Self {
            banks: [[None; 64]; 2],
            owners: [None; 64],
            producer: VecDeque::new(),
            groups: VecDeque::new(),
            pending: None,
            heads: [None; 2],
            read_head: 0,
            pop_head: 0,
            allocate: 0,
            reclaim: 0,
            consumed: 0,
        }
    }
}
impl Pool {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            producer: self.producer.len(),
            unwritten: self.producer.iter().filter(|r| r.written.is_none()).count(),
            groups: self.groups.len()
                + usize::from(self.pending.is_some())
                + self.heads.iter().filter(|h| h.is_some()).count(),
            rows: (self.allocate - self.reclaim) as usize,
            heads: self.heads.iter().filter(|h| h.is_some()).count(),
            pending: self.pending.is_some(),
        }
    }
    pub fn producer_ready(&self) -> bool {
        self.producer.len() < 16
    }
    pub fn idle(&self) -> bool {
        let s = self.snapshot();
        s.producer == 0 && s.groups == 0 && s.rows == 0
    }
    pub fn front(&self) -> Option<i128> {
        self.heads[self.pop_head].map(|(_, w)| w)
    }
    pub fn consume(&mut self, t: u64, payload: i128, events: &mut Vec<Event>) -> Result<(), Error> {
        let (r, w) = self.heads[self.pop_head]
            .take()
            .ok_or("pool consume without old head")?;
        if r.serial != self.consumed || w != payload || !r.written.is_some_and(|edge| edge < t) {
            return Err("pool consume owner/order/value".into());
        }
        events.push(Event::Consume {
            serial: r.serial,
            head: self.pop_head,
            payload,
        });
        self.consumed += 1;
        self.pop_head ^= 1;
        Ok(())
    }
    pub fn reserve(&mut self, owner: Owner, events: &mut Vec<Event>) -> Result<(), Error> {
        if !self.producer_ready() || owner.quad > 15 || owner.lane > 3 {
            return Err("pool producer reservation credit/owner".into());
        }
        let serial = self.allocate;
        let row = (serial & 63) as u8;
        if self.owners[usize::from(row)].is_some() || self.allocate - self.reclaim >= 48 {
            return Err("pool row reservation before capture".into());
        }
        self.owners[usize::from(row)] = Some(serial);
        self.producer.push_back(Ref {
            serial,
            row,
            owner,
            written: None,
        });
        self.allocate += 1;
        events.push(Event::Reserve { serial, row, owner });
        Ok(())
    }
    pub fn write(&mut self, t: u64, payload: i128, events: &mut Vec<Event>) -> Result<u8, Error> {
        if !(0..1_i128 << 72).contains(&payload) {
            return Err("pool payload width".into());
        }
        let r = self
            .producer
            .iter_mut()
            .find(|r| r.written.is_none())
            .ok_or("pool W without reserved producer")?;
        let owner = Owner {
            quad: ((payload >> 66) & 15) as u8,
            lane: ((payload >> 70) & 3) as u8,
        };
        if owner != r.owner || self.owners[usize::from(r.row)] != Some(r.serial) {
            return Err("pool W wrong reserved owner".into());
        }
        for (b, shift) in [0, 36].into_iter().enumerate() {
            if self.banks[b][usize::from(r.row)].is_some() {
                return Err("pool W overwrites live payload".into());
            }
            self.banks[b][usize::from(r.row)] =
                Some(((payload >> shift) & ((1_i128 << 36) - 1)) as u64);
        }
        r.written = Some(t);
        events.push(Event::Write {
            serial: r.serial,
            row: r.row,
            payload,
        });
        Ok(r.row)
    }
    // Called after the cache has consumed only an old head. Captured data is
    // never offered to cache on this edge. pre_groups prevents a full32 bypass.
    pub fn advance(
        &mut self,
        t: u64,
        pre_groups: usize,
        write_row: Option<u8>,
        events: &mut Vec<Event>,
    ) -> Result<(), Error> {
        if let Some((mut r, payload, target, edge)) = self.pending.take() {
            if t != edge + 1
                || self.heads[target].is_some()
                || r.serial != self.reclaim
                || self.owners[usize::from(r.row)] != Some(r.serial)
            {
                return Err("pool synchronous return reservation/owner".into());
            }
            let row = usize::from(r.row);
            if self.banks[0][row]
                .map(i128::from)
                .zip(self.banks[1][row])
                .map(|(a, b)| a | i128::from(b) << 36)
                != Some(payload)
            {
                return Err("pool returned data replaced before capture".into());
            }
            r.written = Some(t);
            self.heads[target] = Some((r, payload));
            self.banks[0][row] = None;
            self.banks[1][row] = None;
            self.owners[row] = None;
            self.reclaim += 1;
            events.push(Event::Capture {
                serial: r.serial,
                row: r.row,
                head: target,
                payload,
            });
        }
        if pre_groups < 32
            && self
                .producer
                .front()
                .is_some_and(|r| r.written.is_some_and(|w| w < t))
        {
            let r = self.producer.pop_front().unwrap();
            self.groups.push_back(r);
            events.push(Event::Transfer {
                serial: r.serial,
                row: r.row,
            });
            // Index visibility starts on the next edge, independently of W.
            self.groups.back_mut().unwrap().written = Some(t);
        }
        if self.pending.is_none()
            && self.heads[self.read_head].is_none()
            && self
                .groups
                .front()
                .is_some_and(|r| r.written.is_some_and(|w| w < t))
        {
            let r = self.groups.pop_front().unwrap();
            if write_row == Some(r.row) {
                return Err("pool same-row R/W".into());
            }
            let row = usize::from(r.row);
            let payload = i128::from(self.banks[0][row].ok_or("pool R missing low")?)
                | i128::from(self.banks[1][row].ok_or("pool R missing high")?) << 36;
            if self.owners[row] != Some(r.serial) {
                return Err("pool R stale owner".into());
            }
            self.pending = Some((r, payload, self.read_head, t));
            events.push(Event::Read {
                serial: r.serial,
                row: r.row,
                head: self.read_head,
            });
            self.read_head ^= 1;
        }
        let s = self.snapshot();
        if s.producer > 16
            || s.groups > 32
            || s.rows > 48
            || s.heads + usize::from(s.pending) > 2
            || s.rows != s.producer + s.groups - s.heads
        {
            return Err("pool physical/logical occupancy".into());
        }
        Ok(())
    }
}
