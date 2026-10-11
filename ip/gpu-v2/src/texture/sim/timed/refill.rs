//! Runtime owns one demand descriptor until terminal; generic retains prefetch.
use super::{Active, Descriptor, Error, RefillEvent, RefillPort, TileKey};
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Queued {
    pub pending: VecDeque<Descriptor>,
    pub active: Option<Active>,
}
enum Phase {
    Pending,
    Presented { id: u64 },
    Started { id: u64, next: usize },
}
pub(super) struct Demand {
    descriptor: Descriptor,
    phase: Phase,
}

pub(super) enum Owner {
    Generic(Queued),
    Demand(Option<Demand>),
}
pub(super) enum Action {
    Started,
    Beat {
        line: usize,
        index: usize,
        data: u64,
    },
    Ready(Descriptor),
}
impl Owner {
    pub fn generic() -> Self {
        Self::Generic(Queued::default())
    }
    pub fn demand() -> Self {
        Self::Demand(None)
    }
    pub fn count(&self) -> usize {
        match self {
            Self::Generic(q) => q.pending.len() + usize::from(q.active.is_some()),
            Self::Demand(d) => usize::from(d.is_some()),
        }
    }
    pub fn active(&self) -> bool {
        self.progress().is_some()
    }
    pub fn available(&self, capacity: usize) -> bool {
        match self {
            Self::Generic(_) => self.count() < capacity,
            Self::Demand(d) => d.is_none(),
        }
    }
    pub fn push(&mut self, descriptor: Descriptor) -> Result<(), Error> {
        match self {
            Self::Generic(q) => q.pending.push_back(descriptor),
            Self::Demand(d) => {
                if d.is_some() {
                    return Err("single demand descriptor still owned".into());
                }
                *d = Some(Demand {
                    descriptor,
                    phase: Phase::Pending,
                });
            }
        }
        Ok(())
    }
    pub fn promote(&mut self, key: TileKey) -> bool {
        // The selected owner has no queue, search, remove/reinsert or promotion.
        let Self::Generic(q) = self else {
            return false;
        };
        if let Some(i) = q.pending.iter().position(|d| d.key == key) {
            if i != 0 {
                let d = q.pending.remove(i).unwrap();
                q.pending.push_front(d);
                return true;
            }
        }
        false
    }
    pub fn submit<M: RefillPort + ?Sized>(
        &mut self,
        memory: &mut M,
    ) -> Result<Option<(u64, Descriptor)>, Error> {
        let descriptor = match self {
            Self::Generic(q) if q.active.is_none() => q.pending.pop_front(),
            Self::Demand(Some(d)) if matches!(d.phase, Phase::Pending) => {
                Some(d.descriptor.clone())
            }
            _ => None,
        };
        let Some(descriptor) = descriptor else {
            return Ok(None);
        };
        // This is actual transport presentation, not physical Started/acceptance.
        let id = memory.submit_read(descriptor.address, 128)?;
        match self {
            Self::Generic(q) => {
                q.active = Some(Active {
                    descriptor: descriptor.clone(),
                    id,
                    started: false,
                    next: 0,
                })
            }
            Self::Demand(Some(d)) => d.phase = Phase::Presented { id },
            _ => unreachable!(),
        }
        Ok(Some((id, descriptor)))
    }
    fn progress(&self) -> Option<(&Descriptor, u64, bool, usize)> {
        match self {
            Self::Generic(q) => q
                .active
                .as_ref()
                .map(|a| (&a.descriptor, a.id, a.started, a.next)),
            Self::Demand(Some(d)) => match d.phase {
                Phase::Pending => None,
                Phase::Presented { id } => Some((&d.descriptor, id, false, 0)),
                Phase::Started { id, next } => Some((&d.descriptor, id, true, next)),
            },
            Self::Demand(None) => None,
        }
    }
    fn advance(&mut self, started: bool, next: usize) {
        match self {
            Self::Generic(q) => {
                let a = q.active.as_mut().unwrap();
                a.started = started;
                a.next = next;
            }
            Self::Demand(Some(d)) => {
                let id = match d.phase {
                    Phase::Presented { id } | Phase::Started { id, .. } => id,
                    Phase::Pending => unreachable!(),
                };
                d.phase = Phase::Started { id, next };
            }
            Self::Demand(None) => unreachable!(),
        }
    }
    pub fn response(&mut self, response: RefillEvent, written: bool) -> Result<Action, Error> {
        let (descriptor, id, started, next) = self.progress().ok_or("unsolicited refill event")?;
        match response {
            RefillEvent::Started { id: received } => {
                if received != id || started {
                    return Err("refill start identity/order".into());
                }
                self.advance(true, 0);
                Ok(Action::Started)
            }
            RefillEvent::Beat {
                id: received,
                index,
                data,
                last,
            } => {
                if received != id
                    || !started
                    || index != next
                    || index >= 16
                    || last != (index == 15)
                    || written
                {
                    return Err("refill beat identity/order/port budget".into());
                }
                let line = descriptor.line;
                self.advance(true, next + 1);
                Ok(Action::Beat { line, index, data })
            }
            RefillEvent::Complete { id: received } => {
                if received != id || !started || next != 16 {
                    return Err("refill completion without sixteen beats".into());
                }
                let descriptor = descriptor.clone();
                match self {
                    Self::Generic(q) => q.active = None,
                    Self::Demand(d) => *d = None,
                }
                Ok(Action::Ready(descriptor))
            }
        }
    }
    #[cfg(test)]
    pub fn probe(&self) -> (usize, usize, bool, usize) {
        let active = self.progress();
        let pending = match self {
            Self::Generic(q) => q.pending.len(),
            Self::Demand(Some(d)) => usize::from(matches!(d.phase, Phase::Pending)),
            _ => 0,
        };
        (
            pending,
            usize::from(active.is_some()),
            active.is_some_and(|a| a.2),
            active.map_or(0, |a| a.3),
        )
    }
    #[cfg(test)]
    pub fn generic_mut(&mut self) -> &mut Queued {
        let Self::Generic(q) = self else {
            panic!("generic refill reference")
        };
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Port {
        submits: usize,
    }
    impl RefillPort for Port {
        fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String> {
            assert_eq!((address, bytes), (4096, 128));
            self.submits += 1;
            assert_eq!(self.submits, 1);
            Ok(7)
        }
        fn step(&mut self) -> Result<Vec<RefillEvent>, String> {
            Ok(vec![])
        }
    }
    fn descriptor() -> Descriptor {
        Descriptor {
            key: TileKey {
                slot: 0,
                n: 3,
                x: 0,
                y: 0,
            },
            line: 2,
            address: 4096,
        }
    }
    #[test]
    fn demand_holds_presentation_start_sixteen_beats_and_terminal_owner() {
        let mut owner = Owner::demand();
        let mut port = Port { submits: 0 };
        owner.push(descriptor()).unwrap();
        assert_eq!(owner.probe(), (1, 0, false, 0));
        assert_eq!(owner.submit(&mut port).unwrap().unwrap().0, 7);
        let presented = owner.probe();
        assert_eq!(presented, (0, 1, false, 0));
        for _ in 0..23 {
            assert!(owner.submit(&mut port).unwrap().is_none());
            assert_eq!(owner.probe(), presented);
        }
        assert!(owner
            .response(
                RefillEvent::Beat {
                    id: 7,
                    index: 0,
                    data: 0,
                    last: false
                },
                false
            )
            .is_err());
        assert_eq!(owner.probe(), presented);
        owner
            .response(RefillEvent::Started { id: 7 }, false)
            .unwrap();
        for invalid in [
            RefillEvent::Started { id: 7 },
            RefillEvent::Beat {
                id: 8,
                index: 0,
                data: 0,
                last: false,
            },
            RefillEvent::Beat {
                id: 7,
                index: 1,
                data: 0,
                last: false,
            },
            RefillEvent::Beat {
                id: 7,
                index: 0,
                data: 0,
                last: true,
            },
            RefillEvent::Complete { id: 7 },
        ] {
            let before = owner.probe();
            assert!(owner.response(invalid, false).is_err());
            assert_eq!(owner.probe(), before);
        }
        assert!(owner
            .response(
                RefillEvent::Beat {
                    id: 7,
                    index: 0,
                    data: 0,
                    last: false
                },
                true
            )
            .is_err());
        for index in 0..16 {
            assert!(matches!(
                owner
                    .response(
                        RefillEvent::Beat {
                            id: 7,
                            index,
                            data: index as u64,
                            last: index == 15
                        },
                        false
                    )
                    .unwrap(),
                Action::Beat { line: 2, .. }
            ));
        }
        assert_eq!(owner.probe(), (0, 1, true, 16));
        for _ in 0..23 {
            assert!(!owner.available(4));
            assert!(!owner.promote(descriptor().key));
            assert!(owner.submit(&mut port).unwrap().is_none());
        }
        assert!(owner.push(descriptor()).is_err());
        assert_eq!(owner.probe(), (0, 1, true, 16));
        assert!(owner
            .response(RefillEvent::Complete { id: 8 }, false)
            .is_err());
        assert_eq!(owner.probe(), (0, 1, true, 16));
        assert!(matches!(
            owner
                .response(RefillEvent::Complete { id: 7 }, false)
                .unwrap(),
            Action::Ready(Descriptor { line: 2, .. })
        ));
        assert!(owner.available(4));
        assert_eq!(owner.count(), 0);
        assert!(owner
            .response(RefillEvent::Complete { id: 7 }, false)
            .is_err());
    }
}
