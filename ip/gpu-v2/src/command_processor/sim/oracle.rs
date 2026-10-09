use super::super::ports::*;
pub fn validate(commands: &[Command], max_commands: usize) -> Result<(), String> {
    if commands.is_empty() || commands.len() > max_commands || max_commands > 1024 {
        return Err("command count bound".into());
    }
    for command in commands {
        match command {
            Command::Dma(d) => {
                d.validate()?;
            }
            Command::Wait(token) if *token > 3 => return Err("WAIT token".into()),
            Command::Draw {
                region,
                byte_offset,
                vertices,
                context,
            } => {
                context.validate()?;
                if *region > 1
                    || *vertices == 0
                    || *vertices > 64
                    || *byte_offset % 4 != 0
                    || byte_offset
                        .checked_add(vertices * 12)
                        .is_none_or(|end| end > 4096)
                {
                    return Err("DRAW vertex range".into());
                }
            }
            Command::Release { slot, epoch } if *slot > 1 || *epoch == 0 => {
                return Err("release slot/epoch".into())
            }
            Command::Unsupported(_) => return Err("unsupported command".into()),
            _ => {}
        }
    }
    Ok(())
}
