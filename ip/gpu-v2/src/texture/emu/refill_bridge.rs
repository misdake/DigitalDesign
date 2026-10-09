//! Read-only transaction adapter for the existing committed-return facade.
//! A submit reserves the facade's single parent credit (the acceptance here);
//! physical Started/beat/terminal events still arrive separately. This adapter
//! polls first and never advances an external shared physical clock itself.
use crate::{
    memory::ports::{MemoryPort, Request, Response},
    texture::ports::{RefillEvent, RefillPort},
};
pub struct RefillBridge<M: RefillPort> {
    inner: M,
    parent: Option<u64>,
    started: bool,
}
impl<M: RefillPort> RefillBridge<M> {
    pub fn new(inner: M) -> Self {
        Self {
            inner,
            parent: None,
            started: false,
        }
    }
    pub fn inner(&self) -> &M {
        &self.inner
    }
    pub fn inner_mut(&mut self) -> &mut M {
        &mut self.inner
    }
}
impl<M: RefillPort> MemoryPort for RefillBridge<M> {
    fn cycle(&mut self, request: Option<Request>, write: Option<u64>) -> Result<Response, String> {
        if write.is_some() || request.is_some_and(|q| q.write) {
            return Err("read-only refill bridge write".into());
        }
        let mut result = Response::default();
        for e in self.inner.step()? {
            let id = match e {
                RefillEvent::Started { id }
                | RefillEvent::Beat { id, .. }
                | RefillEvent::Complete { id } => id,
            };
            if self.parent != Some(id) {
                return Err("refill bridge stale/unowned event".into());
            }
            match e {
                RefillEvent::Started { .. } => {
                    if self.started {
                        return Err("refill bridge duplicate start".into());
                    }
                    self.started = true;
                }
                RefillEvent::Beat {
                    index, data, last, ..
                } => {
                    if !self.started
                        || result.read.is_some()
                        || index >= 16
                        || last != (index == 15)
                    {
                        return Err("refill bridge beat cardinality/order".into());
                    }
                    result.read = Some((index as u8, data));
                }
                RefillEvent::Complete { .. } => {
                    if !self.started || result.complete.is_some() {
                        return Err("refill bridge terminal without start".into());
                    }
                    result.complete = Some(true);
                    self.parent = None;
                    self.started = false;
                }
            }
        }
        if let Some(q) = request {
            q.validate()?;
            if self.parent.is_some() {
                return Err("refill bridge exceeded single parent".into());
            }
            self.parent = Some(self.inner.submit_read(q.address_bytes, 128)?);
            self.started = false;
            result.accepted = true;
        }
        Ok(result)
    }
}
