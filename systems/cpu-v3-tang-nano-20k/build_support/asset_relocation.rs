//! System-owned native sector fixups after placement and before loader COPY.
use cpu_v3_tools::applications::asset_metadata::SectorRelocation;
use rcc::frontend::static_assets::StaticAsset;
use std::collections::{HashMap, HashSet};

pub fn apply_retained(
    assets: &mut [StaticAsset],
    discarded: &[StaticAsset],
    records: &[SectorRelocation],
) -> Result<(), String> {
    let retained: HashSet<_> = assets.iter().map(|a| a.name.as_str()).collect();
    let omitted: HashSet<_> = discarded.iter().map(|a| a.name.as_str()).collect();
    for record in records {
        if retained.contains(record.asset.as_str()) && omitted.contains(record.target.as_str()) {
            return Err(format!(
                "live asset {} references discarded {}",
                record.asset, record.target
            ));
        }
    }
    let mut complete = assets.to_vec();
    complete.extend_from_slice(discarded);
    apply(&mut complete, records)?;
    for (asset, patched) in assets.iter_mut().zip(complete) {
        asset.data = patched.data;
    }
    Ok(())
}

pub fn apply(assets: &mut [StaticAsset], records: &[SectorRelocation]) -> Result<(), String> {
    let symbols: HashMap<_, _> = assets
        .iter()
        .enumerate()
        .map(|(i, a)| (a.name.as_str(), i))
        .collect();
    let mut written = HashSet::new();
    let mut patches = vec![];
    for record in records {
        let source = *symbols
            .get(record.asset.as_str())
            .ok_or_else(|| format!("unknown relocation asset {}", record.asset))?;
        let target = *symbols
            .get(record.target.as_str())
            .ok_or_else(|| format!("unknown relocation target {}", record.target))?;
        let data = &assets[source].data;
        if record.offset & 1 != 0
            || !record
                .offset
                .checked_add(2)
                .is_some_and(|end| end <= data.len())
        {
            return Err(format!(
                "relocation is outside {} or not word aligned",
                record.asset
            ));
        }
        if !written.insert((source, record.offset))
            || data[record.offset..record.offset + 2] != [0, 0]
        {
            return Err(format!(
                "relocation in {} is repeated or does not reference a zero placeholder",
                record.asset
            ));
        }
        if record.addend & 127 != 0 || record.addend >= assets[target].data.len() {
            return Err(format!(
                "relocation target in {} is outside its payload or not sector aligned",
                record.target
            ));
        }
        let address = assets[target]
            .address
            .checked_add(record.addend)
            .ok_or("relocation address overflow")?;
        if address & 127 != 0 {
            return Err("relocation target base is not sector aligned".into());
        }
        let sector =
            u16::try_from(address / 128).map_err(|_| "relocation sector address overflow")?;
        patches.push((source, record.offset, sector));
    }
    // Validate the complete set before touching any payload.
    for (source, offset, sector) in patches {
        assets[source].data[offset..offset + 2].copy_from_slice(&sector.to_le_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset(name: &str, address: usize) -> StaticAsset {
        StaticAsset {
            name: name.into(),
            address,
            alignment: 128,
            data: vec![0; 384],
        }
    }
    #[test]
    fn mesh_pointers_follow_placement_and_invalid_sets_are_transactional() {
        let relocation = SectorRelocation {
            asset: "MESH".into(),
            offset: 0,
            target: "MESH".into(),
            addend: 128,
        };
        for base in [0x100000, 0x4e0000, 0x7ffe80] {
            let mut assets = vec![asset("MESH", base)];
            apply(&mut assets, std::slice::from_ref(&relocation)).unwrap();
            assert_eq!(
                u16::from_le_bytes(assets[0].data[..2].try_into().unwrap()) as usize,
                base / 128 + 1
            );
        }
        for change in [
            "unknown",
            "offset",
            "alignment",
            "range",
            "duplicate",
            "overflow",
        ] {
            let mut assets = vec![asset("MESH", 0x100000)];
            let mut bad = relocation.clone();
            match change {
                "unknown" => bad.target = "ABSENT".into(),
                "offset" => bad.offset = 383,
                "alignment" => bad.addend = 1,
                "range" => bad.addend = 384,
                "overflow" => assets[0].address = 0x800000,
                _ => {}
            }
            assert!(
                apply(&mut assets, &[relocation.clone(), bad]).is_err(),
                "{change}"
            );
            assert!(assets[0].data.iter().all(|&v| v == 0));
        }
    }

    #[test]
    fn discarded_payloads_validate_but_cannot_satisfy_live_pointers() {
        let record = SectorRelocation {
            asset: "DEAD".into(),
            target: "DEAD".into(),
            offset: 0,
            addend: 128,
        };
        let mut live = vec![asset("LIVE", 0x100000)];
        let dead = vec![asset("DEAD", 0)];
        apply_retained(&mut live, &dead, std::slice::from_ref(&record)).unwrap();
        let invalid = SectorRelocation {
            offset: 1,
            ..record.clone()
        };
        assert!(apply_retained(&mut live, &dead, &[invalid]).is_err());
        let live_to_dead = SectorRelocation {
            asset: "LIVE".into(),
            ..record
        };
        assert!(apply_retained(&mut live, &dead, &[live_to_dead]).is_err());
    }
}
