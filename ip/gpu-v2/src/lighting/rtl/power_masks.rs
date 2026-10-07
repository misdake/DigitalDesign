//! Exact power-tail rewrite with masks prepared at the drained context boundary.
//! The audited numerical graph and operation ages remain unchanged.
use super::{LoweredFrame, LoweredProgram};
use audited::{Format, Operation};

#[derive(Clone, Copy, Debug)]
pub(super) struct Rewrite {
    pub event: usize,
    pub descriptor: usize,
    pub region: usize,
    pub raw: usize,
}

pub(super) fn encode_descriptor(raw: u64) -> u128 {
    let wide = (1_u128 << ((raw >> 15) & 15)) - 1;
    let fine = (1_u128 << ((raw >> 19) & 15)) - 1;
    u128::from(raw) | (wide << 43) | (fine << 59)
}

fn format(frame: &LoweredFrame, value: usize, bits: u32, signed: bool) -> bool {
    frame.values[value].format
        == (Format {
            bits,
            fraction: 0,
            signed,
        })
}

fn inputs(frame: &LoweredFrame, value: usize, operation: Operation) -> Option<&[usize]> {
    let event = &frame.events[frame.values[value].producer];
    (event.operation == operation).then_some(event.inputs.as_slice())
}

fn match_tail(frame: &LoweredFrame, event: usize) -> Option<Rewrite> {
    let tail = &frame.events[event];
    if tail.operation != Operation::Sub || tail.inputs.len() != 2 {
        return None;
    }
    let raw = tail.inputs[0];
    let aligned = tail.inputs[1];
    if !format(frame, raw, 16, false)
        || !format(frame, aligned, 16, false)
        || !format(frame, tail.output?, 16, false)
    {
        return None;
    }
    let &[index, shift] = inputs(frame, aligned, Operation::Shift)? else {
        return None;
    };
    if !format(frame, shift, 18, true) || !format(frame, index, 16, false) {
        return None;
    }
    let &[source, neg] = inputs(frame, index, Operation::Shift)? else {
        return None;
    };
    let &[zero, positive] = inputs(frame, neg, Operation::Sub)? else {
        return None;
    };
    if source != raw
        || positive != shift
        || !format(frame, neg, 18, true)
        || !format(frame, zero, 18, true)
        || inputs(frame, zero, Operation::Literal).is_none()
        || frame.values[zero].raw != 0
    {
        return None;
    }
    let &[code] = inputs(frame, shift, Operation::Resize)? else {
        return None;
    };
    let &[region, wide, fine] = inputs(frame, code, Operation::Select)? else {
        return None;
    };
    if !format(frame, code, 4, false)
        || !format(frame, wide, 4, false)
        || !format(frame, fine, 4, false)
        || !format(frame, region, 1, false)
    {
        return None;
    }
    let &[descriptor] = inputs(frame, wide, Operation::Slice(15))? else {
        return None;
    };
    if inputs(frame, fine, Operation::Slice(19))? != [descriptor]
        || !format(frame, descriptor, 43, false)
    {
        return None;
    }
    let &[compared, boundary] = inputs(frame, region, Operation::Less)? else {
        return None;
    };
    if compared != raw
        || !format(frame, boundary, 15, false)
        || inputs(frame, boundary, Operation::Slice(0))? != [descriptor]
    {
        return None;
    }
    let read = &frame.events[frame.values[descriptor].producer];
    let Operation::Read { memory, row: 0 } = read.operation else {
        return None;
    };
    if frame.memories[memory].name != "context.power" {
        return None;
    }
    Some(Rewrite {
        event,
        descriptor,
        region,
        raw,
    })
}

pub(super) fn certify(program: &LoweredProgram) -> Result<Rewrite, String> {
    let mut matches = Vec::new();
    for ins in &program.instructions {
        for &event in &ins.members {
            let Some(rewrite) = match_tail(&program.frame, event) else {
                continue;
            };
            // The descriptor is a batch-uniform function input. The local
            // predicate/raw producers must belong to this same atomic cone.
            if !program.stable[rewrite.descriptor]
                || !ins.inputs.contains(&rewrite.descriptor)
                || !ins
                    .members
                    .contains(&program.frame.values[rewrite.region].producer)
                || !ins
                    .members
                    .contains(&program.frame.values[rewrite.raw].producer)
            {
                return Err("power mask operands escaped the certified cone".into());
            }
            // Every old descriptor consumer still sees its original low bits.
            if program.frame.events.iter().any(|consumer| {
                consumer.inputs.contains(&rewrite.descriptor)
                    && !matches!(consumer.operation, Operation::Slice(offset)
                        if offset + program.frame.values[consumer.output.unwrap()].format.bits <= 43)
            }) {
                return Err("power descriptor has an uncertified whole-word consumer".into());
            }
            matches.push(rewrite);
        }
    }
    match matches.as_slice() {
        [rewrite] => Ok(*rewrite),
        _ => Err("power masks require exactly one closed unified power-tail cone".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lighting::{calendars::UnifiedCalendar, format::CONTEXT_RAW, LightingQuantization};

    #[test]
    fn all_descriptor_codes_and_u16_coordinates_keep_the_exact_tail() {
        for code in 0..32 {
            let raw = CONTEXT_RAW.get(code).copied().unwrap_or(0);
            let encoded = encode_descriptor(raw);
            assert_eq!(encoded & ((1_u128 << 43) - 1), u128::from(raw));
            for x in 0..=u16::MAX {
                let coarse = u64::from(x) < raw & 32767;
                let shift = (raw >> if coarse { 15 } else { 19 }) & 15;
                let mask = ((encoded >> if coarse { 43 } else { 59 }) & 65535) as u16;
                let original = u32::from(x) - ((u32::from(x) >> shift) << shift);
                assert_eq!(u32::from(x & mask), original, "code={code},x={x}");
            }
        }
    }

    #[test]
    fn certificate_rejects_changed_shift_sign_field_and_descriptor_owner() {
        let quantization = LightingQuantization::CompensatedFloor;
        let options = UnifiedCalendar::Free.options(quantization);
        let plans = UnifiedCalendar::Free.plans(quantization).unwrap();
        let mut program = LoweredProgram::with_plans(
            crate::lighting::LightingProfile::Fast,
            options,
            Some(&plans),
        )
        .unwrap();
        let rewrite = certify(&program).unwrap();
        let aligned = program.frame.events[rewrite.event].inputs[1];
        let index = program.frame.events[program.frame.values[aligned].producer].inputs[0];
        let neg = program.frame.events[program.frame.values[index].producer].inputs[1];
        let neg_event = program.frame.values[neg].producer;
        program.frame.events[neg_event].operation = Operation::Add;
        assert!(certify(&program).is_err());
        program.frame.events[neg_event].operation = Operation::Sub;
        let shift = program.frame.events[neg_event].inputs[1];
        let code = program.frame.events[program.frame.values[shift].producer].inputs[0];
        let wide = program.frame.events[program.frame.values[code].producer].inputs[1];
        let wide_event = program.frame.values[wide].producer;
        program.frame.events[wide_event].operation = Operation::Slice(14);
        assert!(certify(&program).is_err());
        program.frame.events[wide_event].operation = Operation::Slice(15);
        let descriptor_event = program.frame.values[rewrite.descriptor].producer;
        let Operation::Read { memory, .. } = program.frame.events[descriptor_event].operation
        else {
            panic!("descriptor read");
        };
        program.frame.memories[memory].name = "pixel.power".into();
        assert!(certify(&program).is_err());
    }
}
