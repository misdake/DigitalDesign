//! Small scheduling experiment, excluding common BSRAM load/store overhead.
//! It compares the arithmetic phase under explicit issue and retire limits.
//! These counts are cmodel hypotheses, not fitted area or frequency results.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lanes {
    OneWide,
    TwoWide,
    WideAndSmall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduleTrial {
    pub issue_edges: u32,
    pub last_retire_edge: u32,
    pub arithmetic_products: u32,
    pub peak_ready_backlog: u32,
    /// A VLIW word would need to express each issue edge. The chosen coarse
    /// operation needs one ROM word and a fixed local sequencer instead.
    pub hypothetical_vliw_words: u32,
}

pub fn trial(lanes: Lanes, retire_ports: usize) -> ScheduleTrial {
    assert!((1..=2).contains(&retire_ports));
    let mut wide = 0;
    let mut small = 0;
    let mut edge = 0;
    let mut ready_at = Vec::new();
    while wide < 16 || small < 9 {
        edge += 1;
        match lanes {
            Lanes::OneWide => {
                if wide < 16 {
                    wide += 1;
                } else {
                    small += 1;
                }
                ready_at.push(edge + 2);
            }
            Lanes::TwoWide => {
                for _ in 0..2 {
                    if wide < 16 {
                        wide += 1;
                        ready_at.push(edge + 2);
                    } else if small < 9 {
                        small += 1;
                        ready_at.push(edge + 2);
                    }
                }
            }
            Lanes::WideAndSmall => {
                if wide < 16 {
                    wide += 1;
                    ready_at.push(edge + 2);
                }
                if small < 9 {
                    small += 1;
                    ready_at.push(edge + 1);
                }
            }
        }
    }
    let issue_edges = edge;
    edge = 0;
    let mut retired = 0;
    let mut peak_ready_backlog = 0;
    while retired < ready_at.len() {
        edge += 1;
        let ready = ready_at.iter().filter(|&&ready| ready <= edge).count() - retired;
        peak_ready_backlog = peak_ready_backlog.max(ready as u32);
        retired += ready.min(retire_ports);
    }
    ScheduleTrial {
        issue_edges,
        last_retire_edge: edge,
        arithmetic_products: ready_at.len() as u32,
        peak_ready_backlog,
        hypothetical_vliw_words: issue_edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_arithmetic_edges_expose_retirement_cost() {
        assert_eq!(
            (
                trial(Lanes::OneWide, 1).issue_edges,
                trial(Lanes::OneWide, 1).last_retire_edge
            ),
            (25, 27)
        );
        assert_eq!(
            (
                trial(Lanes::TwoWide, 2).issue_edges,
                trial(Lanes::TwoWide, 2).last_retire_edge
            ),
            (13, 15)
        );
        assert_eq!(
            (
                trial(Lanes::WideAndSmall, 2).issue_edges,
                trial(Lanes::WideAndSmall, 2).last_retire_edge
            ),
            (16, 18)
        );
        assert_eq!(trial(Lanes::TwoWide, 1).last_retire_edge, 27);
        assert_eq!(trial(Lanes::WideAndSmall, 1).arithmetic_products, 25);
    }
}
