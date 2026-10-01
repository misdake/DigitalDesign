//! Guard arithmetic is audited; command topology is external control input.
use super::super::ports::*;
use audited::{Fixed, FrameReport, Model};
pub fn run(commands: &[Command], max_commands: usize) -> Result<FrameReport, String> {
    if commands.is_empty() || commands.len() > max_commands || max_commands > 1024 {
        return Err("command count bound".into());
    }
    // Each command owns a fixed five-field captured input store, not an opcode ABI.
    let mut model = Model::numerical();
    let mut inputs = Vec::new();
    for (i, command) in commands.iter().enumerate() {
        let fields: [u64; 5] = match command {
            Command::Dma(d) => [
                0,
                d.physical_addr,
                d.scratchpad_addr as u64,
                d.byte_count as u64,
                u64::from(d.completion_token),
            ],
            Command::Wait(token) => [1, u64::from(*token), 0, 0, 0],
            Command::Draw {
                region,
                byte_offset,
                vertices,
                context,
            } => {
                context.validate()?;
                [2, *region as u64, *byte_offset as u64, *vertices as u64, 0]
            }
            Command::Release { slot, epoch } => [3, *slot as u64, u64::from(*epoch), 0, 0],
            Command::Fence => [4, 0, 0, 0, 0],
            Command::Unsupported(code) => [u64::from(*code) + 5, 0, 0, 0, 0],
        };
        inputs.push(
            model
                .input::<64, 0, false>(&format!("command.{i}"), &fields.map(i128::from))
                .map_err(|e| format!("{e:?}"))?,
        );
    }
    let address_count = commands
        .iter()
        .try_fold(0_usize, |count, command| {
            count.checked_add(match command {
                Command::Dma(d) => d.byte_count / 8,
                Command::Draw { vertices, .. } => *vertices,
                _ => 0,
            })
        })
        .ok_or("command address count overflow")?;
    if address_count > 4096 {
        return Err("command generated address limit".into());
    }
    let f = model
        .compute("commands", commands.len() * 128 + address_count * 16 + 64)
        .map_err(|e| format!("{e:?}"))?;
    let execute = || -> Result<(), audited::Fault> {
        for (i, (command, input)) in commands.iter().zip(inputs).enumerate() {
            let opcode = f.read(input.at::<0>())?;
            f.require::<true>(f.less(opcode, Fixed::<64, 0, false>::constant::<5>())?)?;
            match command {
                Command::Dma(descriptor) => {
                    let source = f.read(input.at::<1>())?;
                    let dest = f.read(input.at::<2>())?;
                    let count = f.read(input.at::<3>())?;
                    let token = f.read(input.at::<4>())?;
                    f.require::<true>(f.less(token, Fixed::<64, 0, false>::constant::<4>())?)?;
                    f.require::<true>(f.less(Fixed::<64, 0, false>::constant::<0>(), count)?)?;
                    f.require::<false>(f.less(Fixed::<64, 0, false>::constant::<4096>(), count)?)?;
                    for value in [source, dest, count] {
                        f.require::<true>(f.less(
                            f.slice::<3, 0, false, 0>(value)?,
                            Fixed::<3, 0, false>::constant::<1>(),
                        )?)?;
                    }
                    let end = f.add::<65, 0, false>(dest, count)?;
                    f.require::<false>(f.less(Fixed::<65, 0, false>::constant::<8192>(), end)?)?;
                    let last = f.sub_same(end, Fixed::<65, 0, false>::constant::<1>())?;
                    let a = f.slice::<53, 0, false, 12>(f.resize_exact::<65, 0, false>(dest)?)?;
                    let b = f.slice::<53, 0, false, 12>(last)?;
                    f.require::<false>(f.less(a, b)?)?;
                    f.require::<false>(f.less(b, a)?)?;
                    let source_end = f.add::<65, 0, false>(source, count)?;
                    f.require::<true>(f.less(
                        source_end,
                        Fixed::<65, 0, false>::constant::<{ 1_i128 << 64 }>(),
                    )?)?;
                    let mut physical = source;
                    let mut scratchpad = f.resize_exact::<14, 0, false>(dest)?;
                    for beat in 0..descriptor.byte_count / 8 {
                        f.publish(&format!("command.{i}.dma.{beat}.physical"), physical)?;
                        f.publish(&format!("command.{i}.dma.{beat}.scratchpad"), scratchpad)?;
                        physical = f.add_same(physical, Fixed::<64, 0, false>::constant::<8>())?;
                        scratchpad =
                            f.add_same(scratchpad, Fixed::<14, 0, false>::constant::<8>())?;
                    }
                }
                Command::Wait(_) => {
                    f.require::<true>(f.less(
                        f.read(input.at::<1>())?,
                        Fixed::<64, 0, false>::constant::<4>(),
                    )?)?;
                }
                Command::Draw {
                    vertices: vertex_count,
                    ..
                } => {
                    let region = f.read(input.at::<1>())?;
                    let offset = f.read(input.at::<2>())?;
                    let vertices = f.read(input.at::<3>())?;
                    f.require::<true>(f.less(region, Fixed::<64, 0, false>::constant::<2>())?)?;
                    f.require::<true>(f.less(Fixed::<64, 0, false>::constant::<0>(), vertices)?)?;
                    f.require::<false>(f.less(Fixed::<64, 0, false>::constant::<64>(), vertices)?)?;
                    f.require::<true>(f.less(
                        f.slice::<2, 0, false, 0>(offset)?,
                        Fixed::<2, 0, false>::constant::<1>(),
                    )?)?;
                    // Constant 12*count = (count<<3)+(count<<2), no DSP.
                    let v = f.resize_exact::<72, 0, false>(vertices)?;
                    let length = f.add_same(
                        f.shift_left_const::<3, 72, 0, false>(v)?,
                        f.shift_left_const::<2, 72, 0, false>(v)?,
                    )?;
                    let end = f.add::<73, 0, false>(offset, length)?;
                    f.require::<false>(f.less(Fixed::<73, 0, false>::constant::<4096>(), end)?)?;
                    let region = f.resize_exact::<14, 0, false>(region)?;
                    let mut address = f.add_same(
                        f.shift_left_const::<12, 14, 0, false>(region)?,
                        f.resize_exact(offset)?,
                    )?;
                    for vertex in 0..*vertex_count {
                        f.publish(&format!("command.{i}.vertex.{vertex}.address"), address)?;
                        let first = f.shift_left_const::<3, 14, 0, false>(
                            f.resize_exact(f.slice::<11, 0, false, 3>(address)?)?,
                        )?;
                        let second = f.add_same(first, Fixed::<14, 0, false>::constant::<8>())?;
                        f.publish(&format!("command.{i}.vertex.{vertex}.first"), first)?;
                        f.publish(&format!("command.{i}.vertex.{vertex}.second"), second)?;
                        f.publish(
                            &format!("command.{i}.vertex.{vertex}.high_half"),
                            f.slice::<1, 0, false, 2>(address)?,
                        )?;
                        address = f.add_same(address, Fixed::<14, 0, false>::constant::<12>())?;
                    }
                }
                Command::Release { .. } => {
                    f.require::<true>(f.less(
                        f.read(input.at::<1>())?,
                        Fixed::<64, 0, false>::constant::<2>(),
                    )?)?;
                    f.require::<true>(f.less(
                        Fixed::<64, 0, false>::constant::<0>(),
                        f.read(input.at::<2>())?,
                    )?)?;
                }
                _ => {}
            }
            f.publish(&format!("command.{i}.opcode"), opcode)?;
        }
        Ok(())
    };
    execute().map_err(|e| format!("command guard: {e:?}"))?;
    let report = f.finish();
    report.audit().map_err(|e| format!("{e:?}"))?;
    Ok(report)
}
