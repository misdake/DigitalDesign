use super::*;
use digital_design_code::EmulatedComponent;

pub struct CpuV2Instance;
pub struct CpuV2EmuInstance;

impl CpuV2Design for CpuV2Instance {
    type Decoder = CpuDecoder;
    type RegisterRead = CpuRegisterRead;
    type Execute = CpuExecute;
    type Writeback = CpuWriteback;
    type ControlFlow = CpuControlFlow;
}

impl CpuV2Design for CpuV2EmuInstance {
    type Decoder = EmulatedComponent<CpuDecoder, CpuDecoderEmu>;
    type RegisterRead = EmulatedComponent<CpuRegisterRead, CpuRegisterReadEmu>;
    type Execute = EmulatedComponent<CpuExecute, CpuExecuteEmu>;
    type Writeback = EmulatedComponent<CpuWriteback, CpuWritebackEmu>;
    type ControlFlow = EmulatedComponent<CpuControlFlow, CpuControlFlowEmu>;
}
