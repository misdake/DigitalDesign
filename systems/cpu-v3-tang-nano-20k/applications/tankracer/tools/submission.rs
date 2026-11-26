//! Application capture inspection for both native submission forms.
use gpu_v2::system::command::abi::DeviceSubmission;

/// Read current captures, while retaining flat v1 captures as replay inputs.
#[allow(dead_code)] // Shared by the inspection and cycle-replay executables.
pub fn from_capture(source: &serde_json::Value) -> Result<DeviceSubmission, &'static str> {
    let base = source["base"].as_u64().ok_or("Missing CB base")?;
    if base % 128 != 0 || base >= 0x800000 {
        return Err("CB base alignment/range");
    }
    let count = if let Some(count) = source.get("count_word") {
        u16::try_from(count.as_u64().ok_or("Invalid COUNT word")?)
            .map_err(|_| "COUNT word exceeds u16")?
    } else {
        let words = source["words"].as_u64().ok_or("Missing CB size")?;
        if words == 0 || words > 16384 {
            return Err("Flat CB size");
        }
        (words - 1) as u16
    };
    let submission = DeviceSubmission::from_device_words(count, (base / 128) as u16)?;
    if let Some(words) = source.get("words") {
        if submission.segment_count().is_some() || words.as_u64() != Some(u64::from(count) + 1) {
            return Err("Capture COUNT/size mismatch");
        }
    }
    Ok(submission)
}

/// Physical halfword addresses in execution order, with END only at a span's end.
/// State (including meshlet DMA slots) remains live across spans.
pub fn command_addresses(
    memory: &[u16],
    submission: DeviceSubmission,
) -> Result<Vec<usize>, &'static str> {
    let submission =
        DeviceSubmission::from_device_words(submission.count_minus_one, submission.base_sector)?;
    let base = usize::from(submission.base_sector) * 64;
    let spans = if let Some(count) = submission.segment_count() {
        let table = memory
            .get(base..base + 64)
            .ok_or("Truncated segment table")?;
        let mut spans = Vec::with_capacity(count);
        for pair in table[64 - count * 2..].as_chunks::<2>().0 {
            let first = usize::from(pair[1] & 31);
            let end = usize::from(pair[1] >> 5) + 1;
            if first >= end || end > 2048 {
                return Err("Segment slot order/range");
            }
            spans.push((usize::from(pair[0]) * 64 + first * 2, end - first));
        }
        spans
    } else {
        vec![(base, usize::from(submission.count_minus_one) + 1)]
    };
    let mut addresses = Vec::new();
    let last = spans.len() - 1;
    for (index, (first, count)) in spans.into_iter().enumerate() {
        let padded_end = (first + count * 2).next_multiple_of(64);
        if padded_end > memory.len() || padded_end > 0x400000 {
            return Err("Truncated command fetch extent");
        }
        let mut ended = false;
        for at in (first..first + count * 2).step_by(2) {
            addresses.push(at);
            if memory[at + 1] >> 12 == 15 {
                if at + 2 != first + count * 2 {
                    return Err("END precedes the command span boundary");
                }
                ended = true;
                break;
            }
        }
        if index == last && !ended {
            return Err("Final command span has no END");
        }
    }
    Ok(addresses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpu_v2::system::command::abi::{Command, CommandSlice};
    use serde_json::json;

    #[test]
    fn capture_preserves_count_words_and_checks_legacy_extent() {
        let old = from_capture(&json!({"base":128,"words":2})).unwrap();
        assert_eq!(old.count_minus_one, 1);
        let list = from_capture(&json!({"base":128,"count_word":32769})).unwrap();
        assert_eq!(list.segment_count(), Some(2));
        for input in [
            json!({"base":129,"words":2}),
            json!({"base":128,"words":0}),
            json!({"base":128,"words":16385}),
            json!({"base":128,"count_word":65536}),
            json!({"base":128,"count_word":32768,"words":32769}),
            json!({"base":128,"count_word":1,"words":3}),
            json!({"base":128,"count_word":null}),
        ] {
            assert!(from_capture(&input).is_err(), "{input}");
        }
    }

    #[test]
    fn list_order_unaligned_ranges_and_intermediate_end_are_preserved() {
        let mut memory = vec![0; 512];
        let spans = [
            CommandSlice::new(264, 8).unwrap(),
            CommandSlice::new(516, 12).unwrap(),
            CommandSlice::new(264, 8).unwrap(),
        ];
        let (list, table) = DeviceSubmission::segment_table(128, &spans).unwrap();
        for (at, pair) in table.as_chunks::<2>().0.iter().enumerate() {
            memory[64 + at] = u16::from_le_bytes(*pair);
        }
        memory[135] = (Command::END.0 >> 16) as u16;
        // Intermediate END advances to the next descriptor, preserving state.
        assert_eq!(
            command_addresses(&memory, list).unwrap(),
            [132, 134, 258, 260, 262, 132, 134]
        );
        // A final slice without END must not count as a completed submission.
        memory[135] = 0;
        assert!(command_addresses(&memory, list).is_err());
        assert!(command_addresses(&memory[..100], list).is_err());
    }

    #[test]
    fn early_end_is_rejected_instead_of_counting_unexecuted_commands() {
        let flat = DeviceSubmission::new(0, 16).unwrap();
        let mut memory = vec![0; 64];
        memory[1] = 0xf000;
        assert!(command_addresses(&memory, flat).is_err());
    }

    #[test]
    fn inspection_covers_cross_sector_draws_and_rejects_unreadable_padding() {
        let flat = DeviceSubmission::new(0, 132).unwrap();
        let mut memory = vec![0; 128];
        memory[65] = 0xf000;
        assert_eq!(command_addresses(&memory, flat).unwrap().len(), 33);
        assert!(command_addresses(&memory[..66], flat).is_err());
    }
}
