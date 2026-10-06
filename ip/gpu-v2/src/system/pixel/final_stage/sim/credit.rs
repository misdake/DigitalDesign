//! Bounded old-state credit/FIFO calendar for the final-color leaf.
//!
//! Finiteness is real: the leaf has [`super::super::PIPELINE_LATENCY`] (= 9)
//! one-operation stages and only [`super::super::RESULT_CAPACITY`] (= 4)
//! outstanding result credits. The arithmetic span (`latency + n - 1`) is only
//! a pipelining lower bound. The engine also stalls whenever all four credits
//! are reserved, so the enabled-edge count until the last result is retired is
//! strictly larger than that bound.
//!
//! This module carries *occupancy only*: which pipeline slot holds a live
//! pixel, how many results wait in the output FIFO and how many credits are
//! outstanding. It never evaluates pixel arithmetic, stores no pixel value and
//! contains no numerical oracle. Every field is derived from the old state, so
//! an accepted pixel can never be funded by a credit returned on the same edge.
//!
//! Edge convention (identical to `emu::FinalEmu` and the emitted RTL):
//!
//! - A pixel is accepted on an enabled edge iff a credit is free *before* the
//!   edge and the input is offered. Acceptance never looks at a same-edge
//!   return.
//! - The accepted pixel enters stage 1. Every enabled edge advances each stage
//!   one step; after `latency` enabled edges the oldest value reaches stage
//!   `latency` and is published into the output FIFO at the end of that edge.
//! - The published result is visible at the FIFO head on the following enabled
//!   edge; it transfers iff the consumer asserts `out_ready`. A transfer
//!   returns one credit.
//! - `ce=0` advances nothing; those edges are not counted here at all.

use super::super::{PIPELINE_LATENCY, RESULT_CAPACITY};

/// One enabled-edge transition of the occupancy calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreditStep {
    /// 1-based count of enabled edges observed since `new`.
    pub enabled_edge: u64,
    pub accepted: bool,
    pub published: bool,
    pub consumed: bool,
    pub credits: usize,
    pub queued: usize,
    pub in_flight: usize,
}

/// Finite-engine completion produced by draining a whole batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CreditCompletion {
    pub offered: usize,
    pub accepted: usize,
    pub retired: usize,
    /// Enabled edges from the first request to the last retirement.
    pub enabled_edges: u64,
    pub first_accept_edge: Option<u64>,
    pub first_publish_edge: Option<u64>,
    pub first_retire_edge: Option<u64>,
    pub max_in_flight: usize,
    pub max_queued: usize,
    /// Enabled edges after the first retirement that transferred no result.
    /// It is non-zero here because four credits cannot keep a nine-stage
    /// pipeline full: the engine accepts a burst of four, then stalls until a
    /// credit returns. This is the finite-capacity bubble, not a bug.
    pub steady_bubbles: u64,
}

/// Old-state occupancy model of the pipeline plus result credits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreditCalendar {
    latency: usize,
    capacity: usize,
    /// `pipe[0]` is stage 1, `pipe[latency - 1]` is the publishing stage. A
    /// slot is true while it holds a live pixel.
    pipe: Vec<bool>,
    fifo: usize,
    credits: usize,
    enabled: u64,
}
impl CreditCalendar {
    pub fn new(latency: usize, capacity: usize) -> Result<Self, String> {
        if latency == 0 || capacity == 0 || capacity > 64 || latency > 4096 {
            return Err("final credit calendar bounds".into());
        }
        Ok(Self {
            latency,
            capacity,
            pipe: vec![false; latency],
            fifo: 0,
            credits: 0,
            enabled: 0,
        })
    }
    /// Calendar bound to this leaf's fixed nine-stage, four-credit engine.
    pub fn leaf() -> Self {
        Self::new(PIPELINE_LATENCY, RESULT_CAPACITY).expect("fixed final-stage geometry")
    }
    pub fn latency(&self) -> usize {
        self.latency
    }
    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn credits(&self) -> usize {
        self.credits
    }
    pub fn queued(&self) -> usize {
        self.fifo
    }
    pub fn in_flight(&self) -> usize {
        self.pipe.iter().filter(|s| **s).count()
    }
    pub fn idle(&self) -> bool {
        self.credits == 0 && self.fifo == 0 && self.in_flight() == 0
    }

    /// Advance exactly one enabled edge from the old state. `accept_request`
    /// is whether a pixel is offered; `out_ready` is the consumer's readiness.
    pub fn step(&mut self, accept_request: bool, out_ready: bool) -> CreditStep {
        let accepted = accept_request && self.credits < self.capacity;
        let published = self.pipe[self.latency - 1];
        let consumed = out_ready && self.fifo > 0;
        // Old-state bookkeeping: the return funds nothing on this edge.
        if consumed {
            self.fifo -= 1;
            self.credits -= 1;
        }
        if published {
            self.fifo += 1;
        }
        if accepted {
            self.credits += 1;
        }
        for i in (1..self.latency).rev() {
            self.pipe[i] = self.pipe[i - 1];
        }
        self.pipe[0] = accepted;
        self.enabled += 1;
        debug_assert_eq!(self.credits, self.fifo + self.in_flight());
        CreditStep {
            enabled_edge: self.enabled,
            accepted,
            published,
            consumed,
            credits: self.credits,
            queued: self.fifo,
            in_flight: self.in_flight(),
        }
    }

    /// Best-case finite completion for `n` pixels: a request is offered on
    /// every enabled edge until all are accepted, and the consumer never
    /// backpressures. Bounded by `max_edges`; no arithmetic is evaluated.
    ///
    /// With the leaf's `latency = 9`, `capacity = 4` the accepted burst is four
    /// pixels and the period is `latency + 2 = 11` enabled edges (a credit
    /// returned at the end of an edge funds the next edge only), so the steady
    /// rate is `4/11`, not one per clock.
    pub fn completion(&self, n: usize, max_edges: u64) -> Result<CreditCompletion, String> {
        if n == 0 {
            return Err("final credit completion requires a pixel".into());
        }
        if max_edges == 0 || max_edges > 4_000_000 {
            return Err("final credit completion bound".into());
        }
        let mut cal = self.clone();
        let mut done = CreditCompletion::default();
        for edge in 1..=max_edges {
            let step = cal.step(done.offered < n, true);
            if step.accepted {
                done.offered += 1;
                done.first_accept_edge.get_or_insert(edge);
            }
            if step.published && done.first_publish_edge.is_none() {
                done.first_publish_edge = Some(edge);
            }
            if step.consumed {
                done.retired += 1;
                done.first_retire_edge.get_or_insert(edge);
            } else if done.first_retire_edge.is_some() {
                done.steady_bubbles += 1;
            }
            done.max_in_flight = done.max_in_flight.max(step.in_flight);
            done.max_queued = done.max_queued.max(step.queued);
            if done.offered == n && done.retired == n {
                done.accepted = done.offered;
                done.enabled_edges = edge;
                return Ok(done);
            }
        }
        Err("final credit calendar deadline".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credit_return_never_funds_the_transfer_edge() {
        let mut cal = CreditCalendar::leaf();
        // Fill all four credits: accepted edges 1..=4.
        for edge in 1..=4 {
            let step = cal.step(true, false);
            assert!(step.accepted, "edge {edge}");
        }
        assert_eq!(cal.credits(), RESULT_CAPACITY);
        // Freeze the input; the four results publish at edges 10..=13.
        for _ in 0..9 {
            cal.step(false, false);
        }
        assert_eq!(cal.queued(), RESULT_CAPACITY);
        assert_eq!(cal.credits(), RESULT_CAPACITY);
        // The edge that transfers one result must not accept another.
        let blocked = cal.step(true, true);
        assert!(blocked.consumed);
        assert!(!blocked.accepted, "credit reused on the same edge");
        assert_eq!(cal.queued(), RESULT_CAPACITY - 1);
        // The freed credit funds the next edge only.
        let funded = cal.step(true, false);
        assert!(funded.accepted);
    }

    #[test]
    fn completion_is_credit_aware_and_steady_below_one_per_clock() {
        // Closed form for this fixed geometry (latency 9, capacity 4): a burst
        // of four accepts every eleven enabled edges.
        let expected = |n: u64| 11 * n.div_ceil(4) + (n - 1) % 4;
        let cal = CreditCalendar::leaf();
        for n in 1..=500u64 {
            let done = cal.completion(n as usize, 100_000).unwrap();
            assert_eq!(done.accepted, n as usize);
            assert_eq!(done.retired, n as usize);
            assert_eq!(done.enabled_edges, expected(n), "n={n}");
            assert!(done.max_in_flight <= RESULT_CAPACITY);
            assert!(done.max_queued <= RESULT_CAPACITY);
        }
        // Long continuous requests: observed rate is the capacity-bound 4/11,
        // strictly below one pixel per enabled edge.
        let long = cal.completion(10_000, 4_000_000).unwrap();
        assert!(long.steady_bubbles > 0);
        assert_eq!(long.enabled_edges, 27_503);
        assert!(long.enabled_edges > (PIPELINE_LATENCY + 10_000 - 1) as u64);
    }

    #[test]
    fn geometry_bounds_are_explicit() {
        assert!(CreditCalendar::new(0, 4).is_err());
        assert!(CreditCalendar::new(9, 0).is_err());
        assert!(CreditCalendar::leaf().completion(0, 100).is_err());
        assert!(CreditCalendar::leaf().completion(1, 0).is_err());
    }
}
