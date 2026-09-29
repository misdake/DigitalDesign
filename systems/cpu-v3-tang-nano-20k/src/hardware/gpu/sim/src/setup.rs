//! Clocked triangle input assembly. Screen projection and edge equations are
//! deliberately outside this boundary while their arithmetic is unsettled.

use std::collections::VecDeque;

use crate::result_store::{ResultRead, ResultStore, TransformedVertex, TriangleRef};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TriangleSetupInput {
    pub refs: TriangleRef,
    pub vertices: [TransformedVertex; 3],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupStep {
    pub read_row: Option<usize>,
    pub published: bool,
    pub stalled: bool,
}

#[derive(Clone, Debug)]
enum Phase {
    Idle,
    Read {
        refs: TriangleRef,
        next: usize,
        rows: [u128; 9],
    },
    Publish(TriangleSetupInput),
}

#[derive(Clone, Debug)]
pub struct SetupEngine {
    phase: Phase,
    output: VecDeque<TriangleSetupInput>,
    capacity: usize,
    pub edges: u64,
}

impl SetupEngine {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            phase: Phase::Idle,
            output: VecDeque::new(),
            capacity,
            edges: 0,
        }
    }

    pub fn output(&self) -> &VecDeque<TriangleSetupInput> {
        &self.output
    }

    pub fn pop(&mut self) -> Option<TriangleSetupInput> {
        self.output.pop_front()
    }

    pub fn idle(&self) -> bool {
        matches!(self.phase, Phase::Idle)
    }

    pub fn tick(
        &mut self,
        input: &mut VecDeque<TriangleRef>,
        store: &mut ResultStore,
    ) -> SetupStep {
        self.edges += 1;
        let mut step = SetupStep {
            read_row: None,
            published: false,
            stalled: false,
        };
        let current = std::mem::replace(&mut self.phase, Phase::Idle);
        self.phase = match current {
            Phase::Idle => {
                if let Some(refs) = input.pop_front() {
                    assert!(
                        refs.vertices
                            .iter()
                            .all(|&id| store.vertex(usize::from(id)).is_some()),
                        "setup consumed an unpublished vertex"
                    );
                    Phase::Read {
                        refs,
                        next: 0,
                        rows: [0; 9],
                    }
                } else {
                    Phase::Idle
                }
            }
            Phase::Read {
                refs,
                next,
                mut rows,
            } => {
                let row = usize::from(refs.vertices[next / 3]) * 3 + next % 3;
                store.tick(Some(row), None);
                step.read_row = Some(row);
                rows[next] = match store.output() {
                    ResultRead::Data(value) => value,
                    ResultRead::Collision => panic!("setup result read collided"),
                };
                if next == 8 {
                    let vertices = std::array::from_fn(|corner| {
                        TransformedVertex::from_rows(
                            rows[corner * 3..corner * 3 + 3].try_into().unwrap(),
                        )
                    });
                    Phase::Publish(TriangleSetupInput { refs, vertices })
                } else {
                    Phase::Read {
                        refs,
                        next: next + 1,
                        rows,
                    }
                }
            }
            Phase::Publish(record) => {
                if self.output.len() == self.capacity {
                    step.stalled = true;
                    Phase::Publish(record)
                } else {
                    self.output.push_back(record);
                    step.published = true;
                    Phase::Idle
                }
            }
        };
        step
    }

    pub fn drain(
        &mut self,
        input: &mut VecDeque<TriangleRef>,
        store: &mut ResultStore,
        max_edges: u64,
    ) -> Result<u64, &'static str> {
        let start = self.edges;
        while !input.is_empty() || !self.idle() {
            if self.edges - start == max_edges {
                return Err("setup edge limit");
            }
            self.tick(input, store);
        }
        Ok(self.edges - start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::{Q14, Q16};

    fn vertex(id: i32) -> TransformedVertex {
        TransformedVertex {
            clip: [Q16::from_raw(i128::from(id)).unwrap(); 4],
            normal: [Q14::from_raw(i128::from(id)).unwrap(); 3],
            rgba: [id as u8; 4],
        }
    }

    #[test]
    fn reads_nine_rows_then_publishes_and_holds_credit() {
        let mut store = ResultStore::new(0x123456789abcdef_u128);
        for id in 0..3 {
            for (offset, data) in vertex(id).rows().into_iter().enumerate() {
                store.tick(None, Some((id as usize * 3 + offset, data)));
            }
            store.publish(id as usize);
        }
        let refs = TriangleRef {
            vertices: [2, 0, 1],
        };
        let mut input = VecDeque::from([refs, refs]);
        let mut engine = SetupEngine::new(1);
        assert_eq!(engine.tick(&mut input, &mut store).read_row, None);
        for expected in [6, 7, 8, 0, 1, 2, 3, 4, 5] {
            assert_eq!(engine.tick(&mut input, &mut store).read_row, Some(expected));
            assert!(engine.output().is_empty());
        }
        assert!(engine.tick(&mut input, &mut store).published);
        assert_eq!(
            engine.output()[0].vertices,
            [vertex(2), vertex(0), vertex(1)]
        );
        engine.tick(&mut input, &mut store);
        for _ in 0..9 {
            engine.tick(&mut input, &mut store);
        }
        assert!(engine.tick(&mut input, &mut store).stalled);
        assert_eq!(engine.output().len(), 1);
        engine.pop();
        assert!(engine.tick(&mut input, &mut store).published);
        assert_eq!(store.inspect_row(9), 0x123456789abcdef_u128);
    }

    #[test]
    fn unpublished_reference_is_rejected_before_any_result_read() {
        let mut store = ResultStore::new(0x555_u128);
        let mut input = VecDeque::from([TriangleRef {
            vertices: [0, 1, 2],
        }]);
        let mut engine = SetupEngine::new(1);
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            engine.tick(&mut input, &mut store);
        }));
        assert!(rejected.is_err());
        assert_eq!(store.inspect_row(0), 0x555_u128);
    }

    #[test]
    fn drain_limit_stops_before_publication() {
        let mut store = ResultStore::new(0);
        for id in 0..3 {
            for (offset, data) in vertex(id).rows().into_iter().enumerate() {
                store.tick(None, Some((id as usize * 3 + offset, data)));
            }
            store.publish(id as usize);
        }
        let mut input = VecDeque::from([TriangleRef {
            vertices: [0, 1, 2],
        }]);
        let mut engine = SetupEngine::new(1);
        assert_eq!(
            engine.drain(&mut input, &mut store, 10),
            Err("setup edge limit")
        );
        assert!(engine.output().is_empty());
        assert_eq!(engine.drain(&mut input, &mut store, 1), Ok(1));
        assert_eq!(engine.output().len(), 1);
    }
}
