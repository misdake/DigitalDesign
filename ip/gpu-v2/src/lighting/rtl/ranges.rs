//! Conservative raw-integer bounds from types and operations, never samples.
use super::LoweredFrame;
use audited::{Format, Operation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RawRange {
    pub lo: i128,
    pub hi: i128,
}
impl RawRange {
    pub fn format(f: Format) -> Self {
        if f.signed {
            Self {
                lo: -(1_i128 << (f.bits - 1)),
                hi: (1_i128 << (f.bits - 1)) - 1,
            }
        } else {
            Self {
                lo: 0,
                hi: (1_i128 << f.bits) - 1,
            }
        }
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
    fn fit(self, f: Format) -> Self {
        let all = Self::format(f);
        // A narrowing/wrapping conversion can cross zero or change sign. Keep
        // the entire result type rather than clipping away reachable values.
        if self.lo >= all.lo && self.hi <= all.hi {
            self
        } else {
            all
        }
    }
}

pub(super) fn infer_ranges(frame: &LoweredFrame) -> Vec<RawRange> {
    let mut ranges: Vec<_> = frame
        .values
        .iter()
        .map(|v| RawRange::format(v.format))
        .collect();
    for e in &frame.events {
        let Some(v) = e.output else { continue };
        let get = |i: usize| ranges[e.inputs[i]];
        let interval = |lo: i128, hi: i128| Some(RawRange { lo, hi });
        let result = match e.operation {
            Operation::Literal => interval(frame.values[v].raw, frame.values[v].raw),
            Operation::Add => get(0)
                .lo
                .checked_add(get(1).lo)
                .zip(get(0).hi.checked_add(get(1).hi))
                .map(|(lo, hi)| RawRange { lo, hi }),
            Operation::Sub => get(0)
                .lo
                .checked_sub(get(1).hi)
                .zip(get(0).hi.checked_sub(get(1).lo))
                .map(|(lo, hi)| RawRange { lo, hi }),
            Operation::Multiply => {
                let a = get(0);
                let b = get(1);
                [
                    a.lo.checked_mul(b.lo),
                    a.lo.checked_mul(b.hi),
                    a.hi.checked_mul(b.lo),
                    a.hi.checked_mul(b.hi),
                ]
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .map(|x| RawRange {
                    lo: *x.iter().min().unwrap(),
                    hi: *x.iter().max().unwrap(),
                })
            }
            Operation::Resize | Operation::BinaryScale => Some(get(0)),
            Operation::ShiftLeft(n) if n < 127 => {
                let factor = 1_i128 << n;
                get(0)
                    .lo
                    .checked_mul(factor)
                    .zip(get(0).hi.checked_mul(factor))
                    .map(|(lo, hi)| RawRange { lo, hi })
            }
            Operation::Shift if get(1).lo == get(1).hi && (-126..=126).contains(&get(1).lo) => {
                let n = get(1).lo;
                if n < 0 {
                    interval(get(0).lo >> (-n as u32), get(0).hi >> (-n as u32))
                } else {
                    let factor = 1_i128 << n;
                    get(0)
                        .lo
                        .checked_mul(factor)
                        .zip(get(0).hi.checked_mul(factor))
                        .map(|(lo, hi)| RawRange { lo, hi })
                }
            }
            Operation::LeadingZeros => {
                interval(0, i128::from(frame.values[e.inputs[0]].format.bits))
            }
            Operation::RescaleFloor(n) => interval(get(0).lo >> n, get(0).hi >> n),
            // Slice is a bit-vector operation; signed sources may cross the mask
            // boundary, so the output format is the conservative default.
            Operation::Slice(n) if get(0).lo >= 0 && !frame.values[v].format.signed => {
                let r = RawRange {
                    lo: get(0).lo >> n,
                    hi: get(0).hi >> n,
                };
                Some(r.fit(frame.values[v].format))
            }
            Operation::Less | Operation::RoundIncrement(_) => interval(0, 1),
            Operation::Select => Some(get(1).union(get(2))),
            _ => None,
        };
        if let Some(r) = result {
            ranges[v] = r.fit(frame.values[v].format);
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lighting::rtl::{LoweredFrame, LoweredValue};
    use audited::Event;

    #[test]
    fn bounds_follow_structure_and_include_zero_lzd_and_wrapping_resize() {
        let f = |bits, signed| Format {
            bits,
            fraction: 0,
            signed,
        };
        let operations = [
            Operation::Read { memory: 0, row: 0 },
            Operation::LeadingZeros,
            Operation::Literal,
            Operation::Sub,
            Operation::Literal,
            Operation::Shift,
            Operation::Literal,
            Operation::Sub,
            Operation::Resize,
        ];
        let inputs = [
            vec![],
            vec![0],
            vec![],
            vec![2, 1],
            vec![],
            vec![3, 4],
            vec![],
            vec![6, 5],
            vec![7],
        ];
        let formats = [
            f(30, false),
            f(18, true),
            f(18, true),
            f(18, true),
            f(18, true),
            f(18, true),
            f(18, true),
            f(18, true),
            f(4, true),
        ];
        let frame = LoweredFrame {
            values: formats
                .into_iter()
                .enumerate()
                .map(|(producer, format)| LoweredValue {
                    format,
                    raw: match producer {
                        2 => 1,
                        4 => -1,
                        _ => 0,
                    },
                    producer,
                })
                .collect(),
            events: operations
                .into_iter()
                .enumerate()
                .map(|(id, operation)| Event {
                    id,
                    operation,
                    resource: None,
                    lane: None,
                    issue_cycle: 0,
                    ready_cycle: 0,
                    inputs: inputs[id].clone(),
                    control: None,
                    output: Some(id),
                    source: std::panic::Location::caller(),
                })
                .collect(),
            memories: vec![],
            outputs: vec![],
        };
        let r = infer_ranges(&frame);
        assert_eq!(r[1], RawRange { lo: 0, hi: 30 });
        assert_eq!(r[3], RawRange { lo: -29, hi: 1 });
        assert_eq!(r[5], RawRange { lo: -15, hi: 0 });
        assert_eq!(r[7], RawRange { lo: 0, hi: 15 });
        assert_eq!(r[8], RawRange { lo: -8, hi: 7 });
    }
}
