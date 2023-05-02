use crate::cpu_v1::devices::{DeviceReadResult, Devices};
use crate::cpu_v1::{CpuComponent, CpuComponentEmu};
use crate::{input_w, Wire, Wires};

#[derive(Clone)]
pub struct CpuBusInput {
    pub bus_addr_write: Wire,
    pub bus_enable: Wire,
    pub bus_addr: Wires<4>,
    pub reg0_data: Wires<4>,
    pub reg1_data: Wires<4>,
    pub imm: Wires<4>,
}
#[derive(Clone)]
pub struct CpuBusOutput {
    pub bus_out: Wires<4>,
    pub bus_addr_next: Wires<4>,
}

pub struct CpuBus;
impl CpuComponent for CpuBus {
    type Input = CpuBusInput;
    type Output = CpuBusOutput;
    fn build(_input: &Self::Input) -> Self::Output {
        todo!()
    }
}

pub struct CpuBusEmu;
impl CpuComponentEmu<CpuBus> for CpuBusEmu {
    fn init_output(i: &CpuBusInput) -> CpuBusOutput {
        let bus_out = input_w();
        let bus_addr_next = input_w();
        bus_out.set_latency(i.reg0_data.get_max_latency() + 2);
        bus_addr_next.set_latency(i.reg0_data.get_max_latency() + 2);
        CpuBusOutput {
            bus_out,
            bus_addr_next,
        }
    }
    fn execute(input: &CpuBusInput, output: &CpuBusOutput) {
        let bus_addr_write = input.bus_addr_write.get() > 0;
        if bus_addr_write {
            output.bus_addr_next.set_u8(input.reg0_data.get_u8());
        } else {
            output.bus_addr_next.set_u8(input.bus_addr.get_u8());
        }

        let bus_enable = input.bus_enable.get() > 0;
        let bus_addr = input.bus_addr.get_u8();
        let reg0 = input.reg0_data.get_u8();
        let reg1 = input.reg1_data.get_u8();
        let bus_opcode = input.imm.get_u8();
        let is_read = bus_opcode == 0;

        let bus_out: u8;
        let bus_out_latency: u16;

        if bus_enable {
            if is_read {
                let mut out = 0;
                let mut latency = 0;
                Devices::visit(|devices| {
                    let DeviceReadResult {
                        out_data,
                        self_latency,
                    } = devices.read(bus_addr, reg0, reg1);
                    out = out_data;
                    latency = self_latency;
                });
                bus_out = out;
                bus_out_latency = latency;
            } else {
                Devices::visit(|devices| {
                    devices.execute(bus_addr, bus_opcode, reg0, reg1);
                });
                bus_out = 0;
                bus_out_latency = 0;
            }
        } else {
            bus_out = 0;
            bus_out_latency = 0;
        }

        let latency1 = input.bus_enable.get_latency();
        let latency2 = input
            .reg0_data
            .wires
            .iter()
            .map(|w| w.get_latency())
            .max()
            .unwrap();
        let latency = latency1.max(latency2) + bus_out_latency;
        output.bus_out.set_u8(bus_out);
        output
            .bus_out
            .wires
            .iter()
            .for_each(|w| w.set_latency(latency));
    }
}
