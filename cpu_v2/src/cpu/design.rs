use digital_design_code::CircuitComponent;

/// Selects the implementation of each stage in the single-cycle data path.
///
/// The top-level builder will call these stages in data-flow order so gate
/// implementations and external-backed emulators can be mixed safely.
pub trait CpuV2Design {
    type InstructionMemory: CircuitComponent;
    type Decoder: CircuitComponent;
    type RegisterRead: CircuitComponent;
    type Execute: CircuitComponent;
    type DataMemory: CircuitComponent;
    type Writeback: CircuitComponent;
    type ControlFlow: CircuitComponent;
}
