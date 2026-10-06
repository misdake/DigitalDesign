//! Certificate for the fixed Q13 ROM decoder inside the read-cycle boundary.
//! Arithmetic stays in the numerical ledger. This combinational decoder shares
//! the ROM's output age; its timing is qualified together with the emitted core.
use audited::{FrameReport, Operation};
use std::{collections::BTreeSet, sync::OnceLock};

fn signature(f: &FrameReport, value: usize, stored: usize, address: usize) -> String {
    if value == stored {
        return "stored18".into();
    }
    if value == address {
        return "address7".into();
    }
    let v = &f.values[value];
    let e = &f.events[v.producer];
    if e.operation == Operation::Literal {
        return format!("literal:{:?}:{}", v.format, v.raw);
    }
    format!(
        "{:?}:{:?}({})",
        e.operation,
        v.format,
        e.inputs
            .iter()
            .map(|&v| signature(f, v, stored, address))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn canonical() -> &'static str {
    static SHAPE: OnceLock<String> = OnceLock::new();
    SHAPE.get_or_init(|| {
        let mut model = audited::Model::numerical();
        let stored = model.input::<18, 0, false>("stored", &[0]).unwrap();
        let address = model.input::<7, 0, false>("address", &[0]).unwrap();
        let frame = model.compute("Q13 decode certificate", 512).unwrap();
        let arithmetic = super::quantization::Arithmetic {
            frame: &frame,
            policy: Default::default(),
        };
        let s = frame.read(stored.at::<0>()).unwrap();
        let a = frame.read(address.at::<0>()).unwrap();
        super::counted::decode_rsqrt(&arithmetic, s, a, "certificate").unwrap();
        let f = frame.finish();
        let s = f.events[0].output.unwrap();
        let a = f.events[1].output.unwrap();
        let result = f
            .values
            .iter()
            .position(|v| v.name.as_deref() == Some("certificate.rsqrt.decoded"))
            .unwrap();
        signature(&f, result, s, a)
    })
}

/// Check exact decoder topology and private intermediates before removing its
/// separate registered arithmetic sites from the physical resource calendar.
pub(super) fn members(f: &FrameReport) -> Result<BTreeSet<usize>, String> {
    let mut all = BTreeSet::new();
    for (entry, v) in f.values.iter().enumerate().filter(|(_, v)| {
        v.name
            .as_deref()
            .is_some_and(|n| n.ends_with(".rsqrt.decoded"))
    }) {
        let prefix = v
            .name
            .as_deref()
            .unwrap()
            .strip_suffix(".rsqrt.decoded")
            .unwrap();
        let named = |suffix: &str| {
            f.values
                .iter()
                .position(|v| v.name.as_deref() == Some(&format!("{prefix}{suffix}")))
                .ok_or("incomplete Q13 decoder")
        };
        let stored = named(".rsqrt.stored")?;
        let address = named(".rsqrt_address")?;
        let read = &f.events[f.values[stored].producer];
        if !matches!(read.operation,Operation::Read { memory, .. } if f.memories[memory].name=="RSQRT_Q13")
            || read.inputs != [address]
            || signature(f, entry, stored, address) != canonical()
        {
            return Err("Q13 decoder certificate mismatch".into());
        }
        let mut region = BTreeSet::new();
        let mut pending = vec![entry];
        while let Some(value) = pending.pop() {
            if value == stored || value == address {
                continue;
            }
            let e = &f.events[f.values[value].producer];
            if e.operation == Operation::Literal {
                continue;
            }
            if region.insert(e.id) {
                pending.extend(&e.inputs);
            }
        }
        for &id in &region {
            let value = f.events[id].output.ok_or("Q13 decode output")?;
            if value != entry
                && (f.outputs.iter().any(|o| o.value == value)
                    || f.events
                        .iter()
                        .any(|e| e.inputs.contains(&value) && !region.contains(&e.id)))
            {
                return Err("Q13 decoder intermediate escapes".into());
            }
        }
        all.extend(region);
    }
    let reads = f.events.iter().filter(|e| matches!(e.operation,Operation::Read {memory,..} if f.memories[memory].name=="RSQRT_Q13")).count();
    let decoders = f
        .values
        .iter()
        .filter(|v| {
            v.name
                .as_deref()
                .is_some_and(|n| n.ends_with(".rsqrt.decoded"))
        })
        .count();
    if reads != decoders {
        return Err("Q13 read lacks its certified decoder".into());
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoder_certificate_rejects_changed_arithmetic_and_escaping_intermediates() {
        use crate::lighting::{sim::counted, LightingProfile, LightingQuantization};
        let config = counted::Config {
            rsqrt_q13: true,
            ..counted::Config::lit_queue_resource_profile(
                LightingProfile::Fast,
                LightingQuantization::CompensatedFloor,
            )
        };
        let f = counted::hardware_template_with_config(true, config)
            .unwrap()
            .frame;
        assert!(!members(&f).unwrap().is_empty());
        let root = f
            .values
            .iter()
            .find(|v| {
                v.name
                    .as_deref()
                    .is_some_and(|n| n.ends_with(".rsqrt.decoded"))
            })
            .unwrap()
            .producer;
        let mut bad = f.clone();
        bad.events[root].operation = Operation::Sub;
        assert!(members(&bad).unwrap_err().contains("certificate mismatch"));
        let mut bad = f.clone();
        let mut escaped = bad.outputs[0].clone();
        escaped.value = bad.events[root].inputs[0];
        bad.outputs.push(escaped);
        assert!(members(&bad).unwrap_err().contains("intermediate escapes"));
        let mut bad = f.clone();
        bad.values[bad.events[root].output.unwrap()].name = None;
        assert!(members(&bad).unwrap_err().contains("lacks"));
    }
}
