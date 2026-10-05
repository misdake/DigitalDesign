//! Bounded, ordered functional transport. A pump turn is not a hardware clock.
use std::collections::VecDeque;

pub struct Fifo<T> {
    items: VecDeque<T>,
    capacity: usize,
    pub peak: usize,
}
impl<T> Fifo<T> {
    pub fn new(capacity: usize) -> Result<Self, String> {
        if !(1..=64).contains(&capacity) {
            return Err("oracle FIFO capacity must be 1..64".into());
        }
        Ok(Self {
            items: VecDeque::with_capacity(capacity),
            capacity,
            peak: 0,
        })
    }
    pub fn full(&self) -> bool {
        self.items.len() == self.capacity
    }
    pub fn empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn push(&mut self, item: T) -> Result<(), String> {
        if self.full() {
            return Err("oracle FIFO overflow".into());
        }
        self.items.push_back(item);
        self.peak = self.peak.max(self.items.len());
        Ok(())
    }
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }
}
