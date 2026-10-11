//! Selected adjacent production leaves. Generic standalone RTL is unchanged.
pub fn membership() -> &'static str {
    include_str!("compact_membership.v")
}
pub fn packet() -> &'static str {
    include_str!("compact_packet.v")
}
