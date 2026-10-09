//! Normal-path lease connection between two independently owned controllers.
//! Source capture retains its snapshot payload; records retain their own rows.
//! This module owns only one offered identity and one delayed feedback ticket.
//! Cancellation composition and render completion are explicitly excluded.

use super::record_transport as record;
use crate::frontend::source_capture;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Out {
    pub source: Vec<source_capture::Event>,
    pub transport: Vec<record::Event>,
}

/// Minimal reusable connection. `offer` is valid + ticket64 + context64;
/// `feedback` is valid + ticket64, plus an unsupported-cancel bit: 195 logical
/// host-witness bits. The u64
/// identity widths are existing diagnostic contracts, not a fitted FPGA tag.
/// Counters are host diagnostics. No source/record payload is copied here.
#[derive(Default)]
pub struct Connection {
    offer: Option<record::SourceOwner>,
    feedback: Option<u64>,
    accepted: u64,
    last_use: u64,
    delivered: u64,
    offered_edges: u64,
    unsupported_cancel: bool,
}

impl Connection {
    pub fn offer(&self) -> Option<u64> {
        self.offer.map(|owner| owner.ticket)
    }
    pub fn feedback(&self) -> Option<u64> {
        self.feedback
    }
    pub fn accepted(&self) -> u64 {
        self.accepted
    }
    pub fn last_use(&self) -> u64 {
        self.last_use
    }
    pub fn delivered(&self) -> u64 {
        self.delivered
    }
    pub fn offered_edges(&self) -> u64 {
        self.offered_edges
    }

    /// Advance both controllers once on a wall edge. The connection exclusively
    /// supplies `ce`, `source_captured`, and the source consumed-ticket input.
    /// Newly observed offers and last-use feedback cannot transfer on this edge.
    /// Upstream setup may inspect `src.snapshot()` until actual last-use feedback
    /// is consumed. Callers must not separately step either controller or consume
    /// that snapshot while this connection owns it.
    pub fn pump(
        &mut self,
        src: &mut source_capture::Controller,
        tr: &mut record::Controller,
        ce: bool,
        mut action: record::Input,
    ) -> Result<Out, String> {
        if self.unsupported_cancel {
            return Err("cancellation composition requires external drain and recreation".into());
        }
        if action.source_captured.is_some() {
            return Err("source offer is owned by the connection".into());
        }
        let offer_now = self.offer;
        let feedback_now = self.feedback;
        if offer_now.is_some() {
            self.offered_edges += 1;
        }
        let source = src.step(ce, feedback_now)?;
        if source.contains(&source_capture::Event::Cancelled) {
            self.unsupported_cancel = true;
            return Err("source cancellation composition is not implemented".into());
        }
        if let Some(ticket) = feedback_now {
            if source.contains(&source_capture::Event::TriangleConsumed { ticket }) {
                self.feedback = None;
                self.delivered += 1;
            } else if ce {
                return Err(format!("source capture did not consume feedback {ticket}"));
            }
        }
        for event in &source {
            if let source_capture::Event::SourceCaptured { ticket, .. } = event {
                if self.offer.is_some() {
                    return Err("source capture latched a second offer".into());
                }
                let snapshot = src.snapshot().ok_or("capture without owned snapshot")?;
                if snapshot.ticket != *ticket {
                    return Err("source capture snapshot identity mismatch".into());
                }
                self.offer = Some(record::SourceOwner {
                    ticket: *ticket,
                    context: snapshot.task.context,
                });
            }
        }
        action.ce = ce;
        action.source_captured = offer_now;
        let transport = tr
            .step(action)
            .map_err(|e| format!("record transport fault {e:?}"))?;
        for event in &transport {
            match event {
                record::Event::SnapshotAccepted(owner) => {
                    if offer_now != Some(*owner) {
                        return Err("transport accepted an unoffered identity".into());
                    }
                    self.offer = None;
                    self.accepted += 1;
                }
                record::Event::SnapshotLastUseAck(owner) => {
                    let snapshot = src.snapshot().ok_or("last use without source snapshot")?;
                    if snapshot.ticket != owner.ticket || snapshot.task.context != owner.context {
                        return Err("last use mismatched the source snapshot".into());
                    }
                    if self.feedback.is_some() {
                        return Err("transport repeated last use with feedback pending".into());
                    }
                    self.feedback = Some(owner.ticket);
                    self.last_use += 1;
                }
                record::Event::CancelStarted | record::Event::SnapshotAborted(_) => {
                    self.unsupported_cancel = true;
                    return Err("record cancellation composition is not implemented".into());
                }
                _ => {}
            }
        }
        Ok(Out { source, transport })
    }
}
