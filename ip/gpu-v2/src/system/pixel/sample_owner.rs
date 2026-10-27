//! Public Sampling keys to global destinations. Sampling alone owns lane completion;
//! this adapter retains only the destination high bit and validity per public ID.
pub const DESTINATION_BITS: usize = 16 * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultBeat {
    pub key: u8,
    /// From the actual last public Color consumption, not preparation release.
    pub quad_last: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub reset: bool,
    pub ce: bool,
    pub admission: Option<u8>,
    pub result: Option<ResultBeat>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub accepted: bool,
    pub result_key: Option<u8>,
    pub fault: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Owner {
    valid: u16,
    high: u16,
    fault: bool,
}

impl Owner {
    pub fn input_ready(&self, slot: u8) -> bool {
        !self.fault && slot < 32 && self.valid & (1 << (slot & 15)) == 0
    }

    pub fn destination(&self, key: u8) -> Option<u8> {
        if self.fault || key >= 64 || self.valid & (1 << (key / 4)) == 0 {
            return None;
        }
        Some(key | (((self.high >> (key / 4)) as u8 & 1) << 6))
    }

    pub fn idle(&self) -> bool {
        !self.fault && self.valid == 0
    }

    /// Admission eligibility and destination lookup both observe old state.
    /// A blocked offer is legal; it cannot borrow its alias's release this edge.
    pub fn tick(&mut self, t: Tick) -> Step {
        if t.reset {
            *self = Self::default();
            return Step::default();
        }
        if !t.ce || self.fault {
            return Step {
                fault: self.fault,
                ..Default::default()
            };
        }
        let result_key = t.result.and_then(|r| self.destination(r.key));
        if t.admission.is_some_and(|s| s >= 32) || (t.result.is_some() && result_key.is_none()) {
            self.fault = true;
            return Step {
                fault: true,
                ..Default::default()
            };
        }
        let admission = t.admission.filter(|&s| self.input_ready(s));
        if let Some(r) = t.result.filter(|r| r.quad_last) {
            self.valid &= !(1 << (r.key / 4));
        }
        if let Some(s) = admission {
            let bit = 1 << (s & 15);
            self.valid |= bit;
            self.high = (self.high & !bit) | if s & 16 != 0 { bit } else { 0 };
        }
        Step {
            accepted: admission.is_some(),
            result_key,
            fault: false,
        }
    }
}

/// This is the real adapter controller, with no payload or remaining-mask copy.
/// result_valid means an actual result write/consumption event supplied by the parent.
pub fn verilog() -> &'static str {
    r#"module gpu_v2_pixel_sample_owner(
 input wire clk,reset,ce,
 input wire in_valid,input wire [4:0] in_slot,
 output wire in_ready,input wire result_valid,result_quad_last,
 input wire [5:0] result_key,
 output wire result_owned,output wire [6:0] global_key,
 output wire accepted,write_fire,output reg fault);
 reg [15:0] valid,high;
 wire [3:0] result_id=result_key[5:2];
 wire bad=ce && !reset && !fault && result_valid && !valid[result_id];
 assign in_ready=!reset && !fault && !valid[in_slot[3:0]];
 assign result_owned=!reset && !fault && valid[result_id];
 assign global_key={high[result_id],result_key};
 assign accepted=ce && in_valid && in_ready && !bad;
 assign write_fire=ce && result_valid && result_owned && !bad;
 always @(posedge clk)begin
  if(reset)begin valid<=0;high<=0;fault<=0;end
  else if(ce && !fault)begin
   if(bad)fault<=1;
   else begin
    if(write_fire && result_quad_last)valid[result_id]<=0;
    if(accepted)begin valid[in_slot[3:0]]<=1;high[in_slot[3:0]]<=in_slot[4];end
   end
  end
 end
endmodule
"#
}
