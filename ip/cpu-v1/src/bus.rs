use crate::device::{DeviceReadResult, SharedDeviceBus};
use crate::{CpuComponent, CpuComponentEmu};
use digital_design_circuit::*;

#[derive(Clone)]
pub struct CpuBusInput {
    pub bus_addr0_write: Wire,
    pub bus_addr1_write: Wire,
    pub bus_enable: Wire,
    pub bus_addr0: Wires<4>,
    pub bus_addr1: Wires<4>,
    pub reg0_data: Wires<4>,
    pub reg1_data: Wires<4>,
    pub imm: Wires<4>,
    pub devices: SharedDeviceBus,
}
#[derive(Clone)]
pub struct CpuBusOutput {
    pub bus_out: Wires<4>,
    pub bus_addr0_next: Wires<4>,
    pub bus_addr1_next: Wires<4>,
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
    fn create(i: &CpuBusInput) -> (Self, CpuBusOutput) {
        let bus_out = input_w();
        let bus_addr0_next = input_w();
        let bus_addr1_next = input_w();
        bus_out.set_latency_external(i.reg0_data.get_max_latency_external() + 2);
        bus_addr0_next.set_latency_external(i.reg0_data.get_max_latency_external() + 2);
        bus_addr1_next.set_latency_external(i.reg0_data.get_max_latency_external() + 2);
        (
            Self,
            CpuBusOutput {
                bus_out,
                bus_addr0_next,
                bus_addr1_next,
            },
        )
    }
    fn execute(&mut self, c: &mut CircuitWires, input: &CpuBusInput, output: &CpuBusOutput) {
        let bus_addr0_write = input.bus_addr0_write.get(c) > 0;
        let bus_addr1_write = input.bus_addr1_write.get(c) > 0;
        let bus_addr0_src = select(bus_addr0_write, input.reg0_data, input.bus_addr0);
        let bus_addr1_src = select(bus_addr1_write, input.reg0_data, input.bus_addr1);
        output.bus_addr0_next.set_u8(c, bus_addr0_src.get_u8(c));
        output.bus_addr1_next.set_u8(c, bus_addr1_src.get_u8(c));

        let bus_enable = input.bus_enable.get(c) > 0;
        let reg0 = input.reg0_data.get_u8(c);
        let reg1 = input.reg1_data.get_u8(c);
        let imm = input.imm.get_u8(c); // high 1 bit -> bus0 or bus1, low 3 bit -> opcode

        let bus0_enable = (imm & (0b1000)) == 0;
        let bus1_enable = (imm & (0b1000)) > 0;
        let bus_opcode = imm & 0b0111;

        let bus_addr0 = input.bus_addr0.get_u8(c) * (bus0_enable as u8);
        let bus_addr1 = input.bus_addr1.get_u8(c) * (bus1_enable as u8);
        let bus_addr = bus_addr0 | bus_addr1;

        let bus_out: u8;
        let bus_out_latency: u16;

        if bus_enable {
            let mut devices = input.devices.borrow_mut();

            let DeviceReadResult {
                reg0_write_data: out_data,
                self_latency,
            } = devices.execute(bus_addr, bus_opcode, reg0, reg1);

            bus_out = out_data;
            bus_out_latency = self_latency;
        } else {
            bus_out = 0;
            bus_out_latency = 0;
        }

        let latency1 = input.bus_enable.get_latency(c);
        let latency2 = input
            .reg0_data
            .wires
            .iter()
            .map(|w| w.get_latency(c))
            .max()
            .unwrap();
        let latency = latency1.max(latency2) + bus_out_latency;
        output.bus_out.set_u8(c, bus_out);
        output
            .bus_out
            .wires
            .iter()
            .for_each(|w| w.set_latency(c, latency));
    }
}
