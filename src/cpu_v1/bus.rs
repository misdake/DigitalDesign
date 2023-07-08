use crate::cpu_v1::devices::{DeviceReadResult, Devices};
use crate::cpu_v1::{CpuComponent, CpuComponentEmu};
use crate::{input_w, select, Wire, Wires};
use std::cell::RefCell;
use std::rc::Rc;

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
    pub devices: Rc<RefCell<Devices>>,
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
    fn init_output(i: &CpuBusInput) -> CpuBusOutput {
        let bus_out = input_w();
        let bus_addr0_next = input_w();
        let bus_addr1_next = input_w();
        bus_out.set_latency(i.reg0_data.get_max_latency() + 2);
        bus_addr0_next.set_latency(i.reg0_data.get_max_latency() + 2);
        bus_addr1_next.set_latency(i.reg0_data.get_max_latency() + 2);
        CpuBusOutput {
            bus_out,
            bus_addr0_next,
            bus_addr1_next,
        }
    }
    fn execute(input: &CpuBusInput, output: &CpuBusOutput) {
        let bus_addr0_write = input.bus_addr0_write.get() > 0;
        let bus_addr1_write = input.bus_addr1_write.get() > 0;
        let bus_addr0_src = select(bus_addr0_write, input.reg0_data, input.bus_addr0);
        let bus_addr1_src = select(bus_addr1_write, input.reg0_data, input.bus_addr1);
        output.bus_addr0_next.set_u8(bus_addr0_src.get_u8());
        output.bus_addr1_next.set_u8(bus_addr1_src.get_u8());

        let bus_enable = input.bus_enable.get() > 0;
        let reg0 = input.reg0_data.get_u8();
        let reg1 = input.reg1_data.get_u8();
        let imm = input.imm.get_u8(); // high 1 bit -> bus0 or bus1, low 3 bit -> opcode

        let bus0_enable = (imm & (0b1000)) == 0;
        let bus1_enable = (imm & (0b1000)) > 0;
        let bus_opcode = imm & 0b0111;

        let bus_addr0 = input.bus_addr0.get_u8() * (bus0_enable as u8);
        let bus_addr1 = input.bus_addr1.get_u8() * (bus1_enable as u8);
        let bus_addr = bus_addr0 | bus_addr1;

        let bus_out: u8;
        let bus_out_latency: u16;

        if bus_enable {
            let mut out = 0;
            let mut latency = 0;

            let mut devices = input.devices.borrow_mut();

            let DeviceReadResult {
                reg0_write_data: out_data,
                self_latency,
            } = devices.execute(bus_addr, bus_opcode, reg0, reg1);
            out = out_data;
            latency = self_latency;

            bus_out = out;
            bus_out_latency = latency;
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
