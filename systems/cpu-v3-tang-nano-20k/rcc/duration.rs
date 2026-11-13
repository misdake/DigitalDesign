//! Pure conversions for the u16 millisecond duration domain.
/// A duration in the system's u16 millisecond domain, without silent wrapping.
pub const fn seconds_to_milliseconds(seconds: u16) -> u16 {
    assert!(seconds <= 65);
    seconds * 1000
}
