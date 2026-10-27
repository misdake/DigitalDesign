//! The sole fitted stream profile, also exported to replay fixtures.
use std::{collections::BTreeMap, path::PathBuf};

pub const NAME: &str = "stream-54-108-cl3-v1";
pub const LOGIC_HZ: u32 = 54_000_000;
pub const CONTROLLER_HZ: u32 = 108_000_000;
pub const INIT_CYCLES: u32 = 21_600;

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

pub(crate) fn manifest(files: &BTreeMap<PathBuf, String>, init_cycles: u32) -> String {
    let entries = files
        .iter()
        .map(|(path, text)| {
            format!(
                "\"{}\":{{\"bytes\":{},\"fnv1a64\":\"{:016x}\"}}",
                path.display(),
                text.len(),
                fnv1a64(text.as_bytes())
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"profile\":\"{NAME}\",\"logic_hz\":{LOGIC_HZ},\"controller_hz\":{CONTROLLER_HZ},\"cas\":3,\"rcd\":3,\"rp\":3,\"capture_latency\":3,\"registered_read_return\":1,\"dq_input_delay_taps\":32,\"phase_degrees\":157.5,\"init_cycles\":{init_cycles},\"files\":{{{entries}}}}}\n"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixed_stream_sources_match_the_exported_contract() {
        use super::super::{combination, RtlSources};
        let files = combination::rtl_sources(40).unwrap();
        let contract = &files[std::path::Path::new("stream-contract.json")];
        assert!(contract.contains(super::NAME));
        assert!(contract.contains("\"init_cycles\":40"));
        assert!(RtlSources::STREAM_GEARBOX
            .contains("CAS=3, RCD_CYCLES=3, RP_CYCLES=3, CAPTURE_LATENCY=3"));
        assert!(
            RtlSources::STREAM_GEARBOX.contains("REGISTER_READ_RETURN=1, DQ_INPUT_DELAY_TAPS=32")
        );
        assert!(files[std::path::Path::new("bridge.v")].contains("INIT_CYCLES=40"));
        assert!(!files[std::path::Path::new("combination.v")].contains("__EARLY_GRANT__"));
    }
}
