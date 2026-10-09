use digital_design_hardware_gowin::sdram_memory_controller::ports::*;
use gpu_v2::frontend::ports::MemoryPort;
use std::collections::BTreeMap;
/// Generic test composition: the emu worktree can supply its Service unchanged.
pub struct Adapter<S> {
    pub service: S,
    pub max_cycles: u64,
    pub events: Vec<Event>,
}
impl<S: Service> MemoryPort for Adapter<S> {
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String> {
        if bytes == 0
            || bytes > gpu_v2::scratchpad::ports::REGION_BYTES
            || bytes & 7 != 0
            || address & 7 != 0
        {
            return Err("GPU DMA memory request shape".into());
        }
        let end = address
            .checked_add(bytes as u64)
            .ok_or("GPU DMA address overflow")?;
        let mut current = address & !31;
        let cover_end = end.checked_add(31).ok_or("GPU DMA rounded end overflow")? & !31;
        if cover_end > ADDRESS_BYTES {
            return Err("GPU DMA physical range".into());
        }
        let mut requests = BTreeMap::new();
        let mut result = vec![None; bytes / 8];
        while current < cover_end {
            let remaining = (cover_end - current) as usize;
            let size = [512, 128, 64, 32]
                .into_iter()
                .find(|&size| remaining >= size && current.is_multiple_of(size as u64))
                .unwrap();
            let id = self.service.submit(
                Client::GpuReadOnly,
                Request::Read {
                    address: current,
                    bytes: size,
                },
            )?;
            requests.insert(id, (current, size, 0_usize, false));
            current += size as u64;
            // Complete an accepted burst before admitting the next one. A later
            // source-range error must not leave earlier accepted requests orphaned.
            while requests.values().any(|r| !r.3) {
                if self.service.cycle() >= self.max_cycles {
                    return Err("GPU SDRAM adapter watchdog".into());
                }
                for event in self.service.step()? {
                    match &event {
                        Event::ReadBeat {
                            id,
                            index,
                            data,
                            last,
                            ..
                        } => {
                            let r = requests.get_mut(id).ok_or("unexpected SDRAM read ID")?;
                            if r.3 || *index != r.2 || *last != (*index + 1 == r.1 / 8) {
                                return Err("SDRAM beat order/last".into());
                            }
                            let at = r.0 + *index as u64 * 8;
                            if at >= address && at < end {
                                let slot = &mut result[((at - address) / 8) as usize];
                                if slot.replace(data.bits()).is_some() {
                                    return Err("duplicate DMA source beat".into());
                                }
                            }
                            r.2 += 1;
                        }
                        Event::Complete { id, .. } => {
                            let r = requests
                                .get_mut(id)
                                .ok_or("unexpected SDRAM completion ID")?;
                            if r.3 || r.2 != r.1 / 8 {
                                return Err("SDRAM completion without all data".into());
                            }
                            r.3 = true;
                        }
                        Event::Started { id, .. } => {
                            if !requests.contains_key(id) {
                                return Err("unexpected SDRAM start ID".into());
                            }
                        }
                    }
                    self.events.push(event);
                }
            }
        }
        result
            .into_iter()
            .map(|word| word.ok_or("missing DMA source beat".into()))
            .collect()
    }
}
