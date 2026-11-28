//! Read-only access to original interned identities in the audited source copy.
use super::*;
pub fn encode(symbols: &Symbols) -> Vec<u16> {
    let mut out = vec![u16::try_from(symbols.entries.len()).unwrap()];
    for symbol in &symbols.entries {
        for value in [
            symbol.entity.map(|id| id.0),
            symbol.class.map(|id| u32::try_from(id.0).unwrap()),
        ] {
            let value = value.unwrap_or(u32::MAX);
            out.extend([value as u16, (value >> 16) as u16]);
        }
    }
    out
}
pub fn binding(value: Binding) -> [u16; 3] {
    match value {
        Binding::Direct(entity) => {
            let id = entity.map_or(u32::MAX, |id| id.0);
            [0, id as u16, (id >> 16) as u16]
        }
        Binding::Named(symbol) => [1, u16::try_from(symbol.0).unwrap(), 0],
    }
}
