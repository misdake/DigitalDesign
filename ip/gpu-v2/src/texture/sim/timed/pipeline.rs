//! Two exclusive read consumers. Runtime owns only the one-edge read register;
//! the independent timed reference alone allocates numerical Color state.
use super::*;

pub(super) struct Read {
    pub group: Group4,
    pub line: usize,
}
#[derive(Default)]
pub(super) struct Reference {
    tokens: VecDeque<Token>,
    results: VecDeque<PixelResult>,
    credits: usize,
    accumulator: Option<((u8, u8), [u32; 3])>,
}
pub(super) enum Pipeline {
    Reference(Box<Reference>),
    Capture(Option<Read>),
}
impl Pipeline {
    pub fn is_reference(&self) -> bool {
        matches!(self, Self::Reference(_))
    }
    pub fn idle(&self) -> bool {
        self.pending() == 0 && self.results() == 0
    }
    pub fn pending(&self) -> usize {
        match self {
            Self::Reference(c) => c.tokens.len(),
            Self::Capture(r) => usize::from(r.is_some()),
        }
    }
    pub fn results(&self) -> usize {
        match self {
            Self::Reference(c) => c.results.len(),
            Self::Capture(_) => 0,
        }
    }
    pub fn credits(&self) -> usize {
        match self {
            Self::Reference(c) => c.credits,
            Self::Capture(_) => 0,
        }
    }
    pub fn commit(&mut self) -> Option<PixelResult> {
        match self {
            Self::Reference(c) => {
                let p = c.results.pop_front()?;
                c.credits -= 1;
                Some(p)
            }
            Self::Capture(_) => None,
        }
    }
    pub fn issue(&mut self, group: Group4, payload: i128, line: usize) -> Result<(), Error> {
        match self {
            Self::Reference(c) => {
                c.credits += usize::from(group.last);
                c.tokens.push_back(Token {
                    group,
                    payload,
                    line,
                    age: 0,
                    words: None,
                    partial: None,
                    sum: None,
                });
            }
            Self::Capture(pending) => {
                if pending.is_some() {
                    return Err("cache pending read overwrite".into());
                }
                *pending = Some(Read { group, line });
            }
        }
        Ok(())
    }
    pub fn advance(
        &mut self,
        hardware: &Hardware,
        lines: &[Line; 64],
        banks: &[[u16; 1024]; 4],
        reservations: &mut u64,
        events: &mut Vec<Event>,
    ) -> Result<(), Error> {
        match self {
            Self::Reference(c) => c.advance(hardware, lines, banks, reservations, events),
            Self::Capture(pending) => {
                if let Some(Read { group, line }) = pending.take() {
                    if lines[line].state != State::Ready || lines[line].key != Some(group.key) {
                        return Err("reserved read replaced before capture".into());
                    }
                    let words = std::array::from_fn(|t| {
                        let x = usize::from(group.top_left_local[0]) + (t & 1);
                        let y = usize::from(group.top_left_local[1]) + (t >> 1);
                        let bank = ((y & 1) ^ ((x >> 1) & 1)) * 2 + (x & 1);
                        let local = (y & 7) * 2 + ((x >> 2) & 1);
                        banks[bank][line * 16 + local]
                    });
                    *reservations &= !(1 << line);
                    events.push(Event::Captured { group, line, words });
                }
                Ok(())
            }
        }
    }
}
struct Token {
    group: Group4,
    payload: i128,
    line: usize,
    age: u64,
    words: Option<[u16; 4]>,
    partial: Option<[u32; 3]>,
    sum: Option<[u32; 3]>,
}
impl Reference {
    fn advance(
        &mut self,
        hardware: &Hardware,
        lines: &[Line; 64],
        banks: &[[u16; 1024]; 4],
        reservations: &mut u64,
        events: &mut Vec<Event>,
    ) -> Result<(), Error> {
        let mut next = VecDeque::new();
        while let Some(mut token) = self.tokens.pop_front() {
            token.age += 1;
            if token.age == hardware.read_latency {
                if lines[token.line].state != State::Ready
                    || lines[token.line].key != Some(token.group.key)
                {
                    return Err("reserved read replaced before capture".into());
                }
                let words = std::array::from_fn(|t| {
                    let x = usize::from(token.group.top_left_local[0]) + (t & 1);
                    let y = usize::from(token.group.top_left_local[1]) + (t >> 1);
                    let bank = ((y & 1) ^ ((x >> 1) & 1)) * 2 + (x & 1);
                    let local = (y & 7) * 2 + ((x >> 2) & 1);
                    banks[bank][token.line * 16 + local]
                });
                token.words = Some(words);
                *reservations &= !(1 << token.line);
                events.push(Event::Captured {
                    group: token.group.clone(),
                    line: token.line,
                    words,
                });
            }
            if token.age == hardware.partial_at() {
                let value =
                    filter::partial(token.payload, token.words.ok_or("missing bank data")?)?;
                token.partial = Some(value);
                events.push(Event::Partial {
                    group: token.group.clone(),
                    value,
                });
            }
            if token.age == hardware.accumulator_at() {
                let identity = (token.group.quad_id, token.group.lane);
                let previous = if token.group.first {
                    if self.accumulator.is_some() {
                        return Err("first interrupted prior sample".into());
                    }
                    [0; 3]
                } else {
                    let (owner, value) = self.accumulator.ok_or("non-first without accumulator")?;
                    if owner != identity {
                        return Err("sample group ordering".into());
                    }
                    value
                };
                let value = filter::accumulate(
                    previous,
                    token.partial.ok_or("missing partial")?,
                    token.payload,
                )?;
                token.sum = Some(value);
                events.push(Event::Accumulate {
                    group: token.group.clone(),
                    value,
                });
                self.accumulator = if token.group.last {
                    None
                } else {
                    Some((identity, value))
                };
                if !token.group.last {
                    continue;
                }
            }
            if token.age == hardware.color_latency() {
                let pixel = PixelResult {
                    quad_id: token.group.quad_id,
                    lane: token.group.lane,
                    rgb: filter::normalize(token.sum.ok_or("missing final sum")?)?,
                };
                if self.results.len() >= hardware.result_capacity {
                    return Err("reserved output capacity violated".into());
                }
                self.results.push_back(pixel.clone());
                events.push(Event::Result { pixel });
            } else {
                next.push_back(token);
            }
        }
        self.tokens = next;
        Ok(())
    }
}
