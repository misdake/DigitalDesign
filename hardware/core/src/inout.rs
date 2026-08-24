use digital_design_circuit::Wires;

/// Technology-independent signals surrounding a bidirectional IO buffer.
///
/// This is deliberately only a logical skeleton. A target leaf module owns
/// the physical `inout` pin and maps these signals to the target's IO-buffer
/// primitive. `read` is the resolved pin value, while `write` is driven only
/// where the corresponding `write_enable` bit is asserted.
#[derive(Clone, Copy)]
pub struct InOutSignals<const WIDTH: usize> {
    pub read: Wires<WIDTH>,
    pub write: Wires<WIDTH>,
    pub write_enable: Wires<WIDTH>,
}

impl<const WIDTH: usize> InOutSignals<WIDTH> {
    pub fn new(read: Wires<WIDTH>, write: Wires<WIDTH>, write_enable: Wires<WIDTH>) -> Self {
        assert!(WIDTH > 0, "bidirectional IO width must be non-zero");
        Self {
            read,
            write,
            write_enable,
        }
    }
}
