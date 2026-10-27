//! Real published Sample8 rows feed canonical UV/Raw storage directly.
//! The parent reserves Context8 on raw_write and publishes on committed. It
//! holds the context slot and selected draw config through the four rows and
//! retains the UV lease until C's last read. This unit allocates no contexts.
use super::quad_ingress;
use crate::texture::{
    emu::{uv_rows, uv_store::Coordinate},
    rtl,
};

/// Selected immutable draw fields: bias16/texture-slot4/max-N4/mip1/filter2.
pub fn raw_metadata(config: u32, descriptor: quad_ingress::Descriptor) -> u64 {
    u64::from(config & 0xffff)
        | (u64::from(descriptor.force_coarsest) << 16)
        | (u64::from(descriptor.slot & 15) << 17)
        | (u64::from(descriptor.mask) << 21)
        | (u64::from((config >> 16) & 15) << 25)
        | (u64::from((config >> 20) & 15) << 29)
        | (u64::from((config >> 24) & 1) << 33)
        | (u64::from((config >> 25) & 3) << 34)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub quad: quad_ingress::Tick,
    pub phase: u8,
    pub context_credit: bool,
    pub context_slot: usize,
    pub config: u32,
    pub take_derivative: bool,
    pub coordinate: Option<Coordinate>,
    pub take_coordinate: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Signals {
    pub quad: quad_ingress::Signals,
    pub sample_ready: bool,
    pub next_row: usize,
    pub derivative: Option<(usize, [i64; 8])>,
    pub coordinate: Option<(Coordinate, [u32; 2])>,
    pub coordinate_empty: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub signals: Signals,
    pub quad: quad_ingress::Step,
    pub uv: uv_rows::Edge,
}
pub struct Ingress {
    quad: quad_ingress::Ingress,
    uv: uv_rows::Store,
}
impl Ingress {
    pub fn new(max_wall: u64) -> Self {
        Self {
            quad: quad_ingress::Ingress::new(max_wall),
            uv: uv_rows::Store::new(8).unwrap(),
        }
    }
    pub fn signals(&self, t: Tick) -> Signals {
        let sample_ready = self.uv.input_ready() && (self.uv.next_row() != 0 || t.context_credit);
        let quad = self.quad.signals(quad_ingress::Tick {
            sample_ready,
            ..t.quad
        });
        Signals {
            quad,
            sample_ready: !t.quad.reset && sample_ready,
            next_row: self.uv.next_row(),
            derivative: (!t.quad.reset).then(|| self.uv.derivative()).flatten(),
            coordinate: (!t.quad.reset).then(|| self.uv.coordinate()).flatten(),
            coordinate_empty: self.uv.coordinate_empty(),
        }
    }
    pub fn tick(&mut self, t: Tick) -> Result<Step, String> {
        let signals = self.signals(t);
        let quad = self.quad.tick(quad_ingress::Tick {
            sample_ready: signals.sample_ready,
            ..t.quad
        })?;
        if t.quad.reset {
            self.uv = uv_rows::Store::new(8)?;
            return Ok(Step {
                signals,
                quad,
                uv: uv_rows::Edge::default(),
            });
        }
        let row = quad.sample_accepted.then(|| {
            let row = signals.quad.sample.unwrap();
            let signed = |word: u64| ((word & 0x3ffff) as i64) << 46 >> 46;
            uv_rows::Row {
                slot: t.context_slot,
                metadata: raw_metadata(t.config, row.descriptor),
                uv: [signed(row.data), signed(row.data >> 18)],
            }
        });
        let advance = signals.quad.advance;
        let uv = self.uv.tick(uv_rows::Tick {
            ce: advance,
            phase: t.phase,
            row,
            take_derivative: advance && t.take_derivative,
            coordinate: t.coordinate.filter(|_| advance),
            take_coordinate: advance && t.take_coordinate,
        })?;
        Ok(Step { signals, quad, uv })
    }
}

/// Concrete Quad/Sample8/Basic + UV storage. The reference retains the old
/// three-row assembly and atomic UV-store input, for matched-boundary fitting.
pub fn verilog(
    backend: quad_ingress::rtl::Backend,
    atomic_reference: bool,
) -> Result<String, String> {
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    let head = HEADER.to_owned();
    let consumer = if atomic_reference { ATOMIC } else { ROWS };
    let uv = if atomic_reference {
        rtl::uv_store::source(native)
    } else {
        rtl::uv_rows::source(native)
    };
    Ok(head + QUAD + consumer + "endmodule\n" + &quad_ingress::rtl::verilog(backend)? + "\n" + &uv)
}
const HEADER: &str = r#"module gpu_v2_quad_sampling_ingress(
 input wire clk,
 input wire reset,
 input wire ce,
 input wire in_valid,
 input wire [22:0] in_begin,
 input wire [31:0] in_basic,
 input wire [35:0] in_light,
 input wire [35:0] in_uv,
 output wire in_ready,
 output wire advance,
 output wire [2:0] in_row,
 output wire published,
 output wire [4:0] published_slot,
 output wire [22:0] published_begin,
 output wire basic_write,
 output wire [7:0] basic_write_address,
 output wire [23:0] basic_write_data,
 input wire basic_read,
 input wire [7:0] basic_read_address,
 output wire basic_return_valid,
 output wire [23:0] basic_return_data,
 output wire basic_collision,
 input wire light_ready,
 output wire light_valid,
 output wire [35:0] light_data,
 output wire [2:0] light_row,
 output wire light_last,
 output wire [9:0] light_descriptor,
 output wire sample_ready,
 output wire sample_valid,
 output wire [35:0] sample_data,
 output wire [1:0] sample_row,
 output wire sample_last,
 output wire [10:0] sample_descriptor,
 output wire sample_fire,
 input wire rop_row7_accepted,
 input wire [4:0] rop_slot,
 output wire [4:0] retire_slot,
 output wire [5:0] global_used,
 output wire [1:0] light_used,
 output wire [3:0] sample_used,
 output wire fault,
 input wire [2:0] phase,
 input wire context_credit,
 input wire [2:0] context_slot,
 input wire [26:0] draw_config,
 output wire raw_write,
 output wire [2:0] raw_slot,
 output wire [35:0] raw_metadata,
 output wire committed,
 output wire [2:0] committed_slot,
 input wire d_take,
 output wire d_valid,
 output wire [2:0] d_slot,
 output wire [143:0] d_uv,
 input wire c_request,
 input wire [2:0] c_slot,
 input wire [1:0] c_lane,
 input wire c_take,
 output wire c_empty,
 output wire c_valid,
 output wire [2:0] c_owner_slot,
 output wire [1:0] c_owner_lane,
 output wire [31:0] c_uv,
 output wire uv_write,
 output wire [4:0] uv_write_address,
 output wire [35:0] uv_write_data,
 output wire uv_read,
 output wire [4:0] uv_read_address
);
"#;
const QUAD: &str = r#" gpu_v2_quad_ingress quad(
 .clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_begin(in_begin),
 .in_basic(in_basic),.in_light(in_light),.in_uv(in_uv),.in_ready(in_ready),
 .advance(advance),.in_row(in_row),.published(published),
 .published_slot(published_slot),.published_begin(published_begin),
 .basic_write(basic_write),.basic_write_address(basic_write_address),
 .basic_write_data(basic_write_data),.basic_read(basic_read),
 .basic_read_address(basic_read_address),.basic_return_valid(basic_return_valid),
 .basic_return_data(basic_return_data),.basic_collision(basic_collision),
 .light_ready(light_ready),.sample_ready(sample_ready),.light_valid(light_valid),
 .sample_valid(sample_valid),.light_data(light_data),.sample_data(sample_data),
 .light_row(light_row),.sample_row(sample_row),.light_last(light_last),
 .sample_last(sample_last),.light_descriptor(light_descriptor),
 .sample_descriptor(sample_descriptor),.rop_row7_accepted(rop_row7_accepted),
 .rop_slot(rop_slot),.retire_slot(retire_slot),.global_used(global_used),
 .light_used(light_used),.sample_used(sample_used),.fault(fault));
 wire [35:0] selected_raw={draw_config[26:25],draw_config[24],draw_config[23:20],draw_config[19:16],
   sample_descriptor[8:5],sample_descriptor[3:0],sample_descriptor[10],draw_config[15:0]};
 wire uv_ready;
 assign sample_ready=uv_ready && (sample_row!=0 || context_credit);
 assign sample_fire=advance && sample_valid && sample_ready;
"#;
const ROWS: &str = r#" gpu_v2_uv_rows uv(
 .clk(clk),.reset(reset),.ce(advance),.phase(phase),
 .in_valid(sample_valid && (sample_row!=0 || context_credit)),
 .in_slot(context_slot),.in_metadata(selected_raw),.in_uv(sample_data),
 .in_ready(uv_ready),.in_row(),.raw_write(raw_write),.raw_slot(raw_slot),
 .raw_metadata(raw_metadata),.committed(committed),.committed_slot(committed_slot),
 .d_take(d_take),.d_valid(d_valid),.d_slot(d_slot),.d_uv(d_uv),
 .c_request(c_request),.c_slot(c_slot),.c_lane(c_lane),.c_take(c_take),
 .c_empty(c_empty),.c_valid(c_valid),.c_owner_slot(c_owner_slot),
 .c_owner_lane(c_owner_lane),.c_uv(c_uv),.write_enable(uv_write),
 .write_address(uv_write_address),.write_data(uv_write_data),
 .read_enable(uv_read),.read_address(uv_read_address));
"#;
const ATOMIC: &str = r#" reg [35:0] hold0,hold1,hold2;
 always @(posedge clk) if(!reset && sample_fire) begin
   case(sample_row)
     0: hold0<=sample_data;
     1: hold1<=sample_data;
     2: hold2<=sample_data;
   endcase
 end
 assign raw_write=sample_fire && sample_row==0;
 assign raw_slot=context_slot;
 assign raw_metadata=selected_raw;
 assign committed=sample_fire && sample_row==3;
 assign committed_slot=context_slot;
 gpu_v2_uv_store uv(
 .clk(clk),.reset(reset),.ce(advance),.phase(phase),
 .in_valid(sample_valid && sample_row==3),.in_slot(context_slot),
 .in_uv({sample_data,hold2,hold1,hold0}),.in_ready(uv_ready),
 .d_take(d_take),.d_valid(d_valid),.d_slot(d_slot),.d_uv(d_uv),
 .c_request(c_request),.c_slot(c_slot),.c_lane(c_lane),.c_take(c_take),
 .c_empty(c_empty),.c_valid(c_valid),.c_owner_slot(c_owner_slot),
 .c_owner_lane(c_owner_lane),.c_uv(c_uv),.write_enable(uv_write),
 .write_address(uv_write_address),.write_data(uv_write_data),
 .read_enable(uv_read),.read_address(uv_read_address));
"#;
