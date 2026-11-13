//! Address-independent producer metadata; physical placement belongs to the linker.
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectorRelocation {
    pub asset: String,
    pub offset: usize,
    pub target: String,
    pub addend: usize,
}

pub fn sector_relocations(manifest: &Value) -> Result<Vec<SectorRelocation>, String> {
    let Some(records) = manifest.get("sector_relocations") else {
        return Ok(vec![]);
    };
    records
        .as_array()
        .ok_or("sector_relocations must be an array")?
        .iter()
        .map(|record| {
            let object = record
                .as_object()
                .ok_or("sector relocation must be an object")?;
            if object
                .keys()
                .any(|k| !matches!(k.as_str(), "asset" | "offset" | "target" | "addend"))
            {
                return Err("unknown sector relocation field".into());
            }
            let symbol = |field: &str| -> Result<String, String> {
                let text = object
                    .get(field)
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("sector relocation needs {field}"))?;
                if text.is_empty()
                    || !text.bytes().enumerate().all(|(i, c)| {
                        c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                    })
                {
                    return Err(format!("invalid asset symbol: {text}"));
                }
                Ok(text.into())
            };
            let number = |field: &str| -> Result<usize, String> {
                object
                    .get(field)
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| format!("sector relocation needs unsigned {field}"))
            };
            Ok(SectorRelocation {
                asset: symbol("asset")?,
                offset: number("offset")?,
                target: symbol("target")?,
                addend: number("addend")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn producer_sector_metadata_requires_explicit_symbols_and_byte_offsets() {
        assert!(sector_relocations(&serde_json::json!({}))
            .unwrap()
            .is_empty());
        let record =
            serde_json::json!({"asset":"MESH_BYTES","offset":0,"target":"MESH_BYTES","addend":128});
        let records =
            sector_relocations(&serde_json::json!({"sector_relocations":[record.clone()]}))
                .unwrap();
        assert_eq!(records[0].addend, 128);
        for (field, value) in [
            ("offset", serde_json::json!(-1)),
            ("target", serde_json::json!("not a symbol")),
            ("extra", serde_json::json!(0)),
        ] {
            let mut bad = record.clone();
            bad[field] = value;
            assert!(sector_relocations(&serde_json::json!({"sector_relocations":[bad]})).is_err());
        }
    }
}
