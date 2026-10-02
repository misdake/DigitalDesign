//! Bounded, replayable read-service trace and handshake audit.
//!
//! This is development instrumentation. It checks the public ready/valid
//! contract without inspecting the memory model's private state.

use std::fmt::Write;

use crate::timing::{MemoryCycle, ReadBeat, ReadBurst, ReadMemory};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadTraceRow {
    pub edge: u64,
    pub request: Option<ReadBurst>,
    pub response_ready: bool,
    pub output: MemoryCycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeFault {
    CycleBudget {
        maximum: u64,
    },
    NonSequentialEdge {
        expected: u64,
        actual: u64,
    },
    UnstableRequest {
        edge: u64,
    },
    AcceptedAbsentRequest {
        edge: u64,
    },
    SecondOutstandingRequest {
        edge: u64,
    },
    ResponseWithoutRequest {
        edge: u64,
    },
    UnstableResponse {
        edge: u64,
    },
    TransferMismatch {
        edge: u64,
    },
    WrongLast {
        edge: u64,
        expected: bool,
        actual: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadSummary {
    pub request_edge: u64,
    pub first_beat_edge: u64,
    pub completion_edge: u64,
    pub stalled_response_edges: u64,
    pub beats: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadTrace {
    maximum_edges: u64,
    rows: Vec<ReadTraceRow>,
    held_request: Option<ReadBurst>,
    held_response: Option<ReadBeat>,
    outstanding: Option<(usize, usize)>,
}

impl ReadTrace {
    pub fn new(maximum_edges: u64) -> Self {
        assert!(maximum_edges > 0);
        Self {
            maximum_edges,
            rows: Vec::new(),
            held_request: None,
            held_response: None,
            outstanding: None,
        }
    }

    pub fn rows(&self) -> &[ReadTraceRow] {
        &self.rows
    }

    /// Advance one edge, then check the public bus result. The trace is not
    /// mutated if the observed row violates the protocol.
    pub fn step(
        &mut self,
        memory: &mut ReadMemory,
        request: Option<ReadBurst>,
        response_ready: bool,
    ) -> Result<MemoryCycle, ProbeFault> {
        if self.rows.len() as u64 >= self.maximum_edges {
            return Err(ProbeFault::CycleBudget {
                maximum: self.maximum_edges,
            });
        }
        let output = memory.tick(request, response_ready);
        self.record(ReadTraceRow {
            edge: memory.cycle,
            request,
            response_ready,
            output,
        })?;
        Ok(output)
    }

    /// Audit a row from any future clocked backend using the same bus shape.
    pub fn record(&mut self, row: ReadTraceRow) -> Result<(), ProbeFault> {
        if self.rows.len() as u64 >= self.maximum_edges {
            return Err(ProbeFault::CycleBudget {
                maximum: self.maximum_edges,
            });
        }
        let expected_edge = self.rows.last().map_or(1, |previous| previous.edge + 1);
        if row.edge != expected_edge {
            return Err(ProbeFault::NonSequentialEdge {
                expected: expected_edge,
                actual: row.edge,
            });
        }
        if self.held_request.is_some() && row.request != self.held_request {
            return Err(ProbeFault::UnstableRequest { edge: row.edge });
        }
        if self.held_response.is_some() && row.output.response != self.held_response {
            return Err(ProbeFault::UnstableResponse { edge: row.edge });
        }
        if row.output.response_accepted != (row.response_ready && row.output.response.is_some()) {
            return Err(ProbeFault::TransferMismatch { edge: row.edge });
        }

        let mut outstanding = self.outstanding;
        if row.output.request_accepted {
            let request = row
                .request
                .ok_or(ProbeFault::AcceptedAbsentRequest { edge: row.edge })?;
            if outstanding.is_some() {
                return Err(ProbeFault::SecondOutstandingRequest { edge: row.edge });
            }
            outstanding = Some((request.beats, 0));
        }
        if row.output.response.is_some() && outstanding.is_none() {
            return Err(ProbeFault::ResponseWithoutRequest { edge: row.edge });
        }
        if row.output.response_accepted {
            let beat = row.output.response.expect("accepted response is present");
            let (total, seen) = outstanding.expect("response has an outstanding request");
            let next = seen + 1;
            let expected_last = next == total;
            if beat.last != expected_last {
                return Err(ProbeFault::WrongLast {
                    edge: row.edge,
                    expected: expected_last,
                    actual: beat.last,
                });
            }
            outstanding = if beat.last { None } else { Some((total, next)) };
        }
        self.outstanding = outstanding;
        self.held_request = row.request.filter(|_| !row.output.request_accepted);
        self.held_response = row
            .output
            .response
            .filter(|_| !row.output.response_accepted);
        self.rows.push(row);
        Ok(())
    }

    /// Summary for a completed single-burst trace. The rows remain available
    /// for detailed inspection when a scenario is incomplete.
    pub fn summary(&self) -> Option<ReadSummary> {
        let request_edge = self
            .rows
            .iter()
            .find(|row| row.output.request_accepted)?
            .edge;
        let first_beat_edge = self
            .rows
            .iter()
            .find(|row| row.output.response.is_some())?
            .edge;
        let completion_edge = self
            .rows
            .iter()
            .find(|row| {
                row.output.response_accepted && row.output.response.is_some_and(|beat| beat.last)
            })?
            .edge;
        let stalled_response_edges = self
            .rows
            .iter()
            .filter(|row| row.output.response.is_some() && !row.response_ready)
            .count() as u64;
        let beats = self
            .rows
            .iter()
            .filter(|row| row.output.response_accepted)
            .count();
        Some(ReadSummary {
            request_edge,
            first_beat_edge,
            completion_edge,
            stalled_response_edges,
            beats,
        })
    }

    pub fn render(&self) -> String {
        let mut out = String::from("edge  request    grant  ready  response             take\n");
        for row in &self.rows {
            let request = row.request.map_or(String::from("-"), |burst| {
                format!("{:06x}/{}", burst.address, burst.beats)
            });
            let response = row.output.response.map_or(String::from("-"), |beat| {
                format!("{:016x}{}", beat.data, if beat.last { "L" } else { " " })
            });
            writeln!(
                out,
                "{:>4}  {:>10}  {:>5}  {:>5}  {:>19}  {:>4}",
                row.edge,
                request,
                u8::from(row.output.request_accepted),
                u8::from(row.response_ready),
                response,
                u8::from(row.output.response_accepted),
            )
            .expect("writing to a String cannot fail");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::MemoryTiming;

    #[test]
    fn seeded_latency_trace_replays_and_reports_stalls() {
        let timing = MemoryTiming {
            grant_wait: 2,
            first_beat: 3,
            beat_gap: 1,
            jitter: 2,
        };
        let burst = ReadBurst {
            address: 0,
            beats: 4,
        };
        let mut traces = Vec::new();
        for _ in 0..2 {
            let mut memory = ReadMemory::new(vec![11, 22, 33, 44], timing, 19);
            let mut trace = ReadTrace::new(64);
            let mut pending = true;
            for edge in 1..=64 {
                let output = trace
                    .step(&mut memory, pending.then_some(burst), edge % 3 != 0)
                    .unwrap();
                if output.request_accepted {
                    pending = false;
                }
                if trace.summary().is_some() {
                    break;
                }
            }
            let summary = trace.summary().expect("burst completed before budget");
            assert_eq!(summary.beats, 4);
            assert!(summary.first_beat_edge >= summary.request_edge + 3);
            assert!(summary.stalled_response_edges > 0);
            traces.push(trace);
        }
        assert_eq!(traces[0], traces[1]);
    }

    #[test]
    fn protocol_audit_catches_dropped_request_and_changed_stalled_beat() {
        let burst = ReadBurst {
            address: 0,
            beats: 4,
        };
        let empty = MemoryCycle::default();
        let mut request_trace = ReadTrace::new(4);
        request_trace
            .record(ReadTraceRow {
                edge: 1,
                request: Some(burst),
                response_ready: true,
                output: empty,
            })
            .unwrap();
        assert_eq!(
            request_trace.record(ReadTraceRow {
                edge: 2,
                request: None,
                response_ready: true,
                output: empty
            }),
            Err(ProbeFault::UnstableRequest { edge: 2 })
        );
        assert_eq!(request_trace.rows().len(), 1);

        let mut response_trace = ReadTrace::new(4);
        response_trace
            .record(ReadTraceRow {
                edge: 1,
                request: Some(burst),
                response_ready: true,
                output: MemoryCycle {
                    request_accepted: true,
                    ..empty
                },
            })
            .unwrap();
        response_trace
            .record(ReadTraceRow {
                edge: 2,
                request: None,
                response_ready: false,
                output: MemoryCycle {
                    response: Some(ReadBeat {
                        data: 7,
                        last: false,
                    }),
                    ..empty
                },
            })
            .unwrap();
        assert_eq!(
            response_trace.record(ReadTraceRow {
                edge: 3,
                request: None,
                response_ready: false,
                output: MemoryCycle {
                    response: Some(ReadBeat {
                        data: 8,
                        last: false
                    }),
                    ..empty
                }
            }),
            Err(ProbeFault::UnstableResponse { edge: 3 })
        );
        assert_eq!(response_trace.rows().len(), 2);
    }
}
