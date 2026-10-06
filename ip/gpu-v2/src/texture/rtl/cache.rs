//! Generator for the demand-only texture-cache leaf RTL.
//!
//! The emitted Verilog is a fixed-width, slot-specialized mirror of
//! [`crate::texture::emu::cache::CacheEmu`]. It is produced from the immutable
//! [`Slot`] table so the address base/level ROM is literal wiring, and it
//! instantiates four inferred synchronous bank RAMs. No PnR, BSRAM-fit or
//! timing result is claimed here; the ignored Icarus test drives this module
//! edge for edge against the live emulator through a small behavioral memory.
//!
//! The refill transport handles the request acceptance, the numbered beats and
//! the terminal acknowledgement as three independent events. A terminal may
//! share the last-beat edge or arrive arbitrarily later with no beat; a last
//! beat alone is never an acknowledgement. A same-edge accept+first-beat is
//! supported. Compute uses only the pre-edge register state, so a terminal
//! acknowledgement cannot be spent by a same-edge head read, and a captured read
//! cannot fund a same-edge address issue. `abort` suppresses compute and
//! invalidates a `Filling` publication, but a presented request is held until
//! acceptance and drains to its terminal event.

use crate::texture::emu::cache::{Allocation, BANKS, BANK_DEPTH, LINES, REFILL_BEATS, SETS, WAYS};
use crate::texture::ports::{layer_offset, Slot};

/// Verilog module name emitted by [`verilog`].
pub const MODULE: &str = "gpu_v2_texture_cache";

/// Exact declared capacity of the emitted leaf (shared with the emu).
pub fn allocation() -> Allocation {
    Allocation::default()
}

fn slot_base_literal(slot: Option<&Slot>) -> String {
    format!(
        "32'h{:08x}",
        slot.filter(|s| s.valid).map_or(0, |s| s.base_address)
    )
}

fn mut_verilog(slots: &[Slot]) -> String {
    let base_arms: String = (0..16)
        .map(|s| {
            format!(
                "      4'd{s}: slot_base_f = {};\n",
                slot_base_literal(slots.get(s))
            )
        })
        .collect();
    let full_arms: String = (0..16)
        .map(|s| {
            let full = slots
                .get(s)
                .is_some_and(|slot| slot.valid && slot.has_full_mip);
            format!("      4'd{s}: slot_full_f = 1'b{};\n", u8::from(full))
        })
        .collect();
    let max_arms: String = (0..16)
        .map(|s| {
            let max = slots
                .get(s)
                .filter(|slot| slot.valid)
                .map_or(0, |slot| slot.max_size_log2);
            format!("      4'd{s}: slot_max_f = 4'd{max};\n")
        })
        .collect();
    let valid_arms: String = (0..16)
        .map(|s| {
            format!(
                "      4'd{s}: slot_valid_f = 1'b{};\n",
                u8::from(slots.get(s).is_some_and(|slot| slot.valid))
            )
        })
        .collect();
    let layer_arms: String = (0..=10)
        .map(|n| {
            let value = layer_offset(n).unwrap_or(0);
            format!("      4'd{n}: layer_off_f = 32'd{value};\n")
        })
        .collect();

    format!(
        r#"// {MODULE}: demand-only texture cache, slot-specialized register/RAM leaf.
// Generated from the immutable Slot table; slot base/level values are literal.
// Input is one canonical 72-bit Group4 packet; output is that packet plus four
// RAW565 tap texels. 16 sets x 4 ways, tree-PLRU3, four synchronous 1024x16
// banks, one demand head, one result slot and one 128-byte refill descriptor.
// See texture::emu::cache for the edge-for-edge reference.
module {MODULE}(
  input  wire        clk,
  input  wire        reset,
  input  wire        ce,
  input  wire        abort,
  input  wire        in_valid,
  input  wire [71:0] in_payload,
  output wire        in_ready,
  input  wire        out_ready,
  output wire        out_valid,
  output wire [71:0] out_payload,
  output wire [15:0] out_tex0,
  output wire [15:0] out_tex1,
  output wire [15:0] out_tex2,
  output wire [15:0] out_tex3,
  output wire        mem_req_valid,
  output wire [31:0] mem_req_addr,
  input  wire        mem_req_ready,
  input  wire        mem_resp_valid,
  input  wire [3:0]  mem_resp_index,
  input  wire [63:0] mem_resp_data,
  input  wire        mem_resp_complete,
  input  wire        mem_resp_ok,
  output wire        drained,
  output reg         fault
);
  // Immutable slot parameters and the fixed mip prefix ROM.
  function [31:0] slot_base_f(input [3:0] s);
    case (s)
{base_arms}      default: slot_base_f = 32'd0;
    endcase
  endfunction
  function slot_full_f(input [3:0] s);
    case (s)
{full_arms}      default: slot_full_f = 1'b0;
    endcase
  endfunction
  function [3:0] slot_max_f(input [3:0] s);
    case (s)
{max_arms}      default: slot_max_f = 4'd0;
    endcase
  endfunction
  function slot_valid_f(input [3:0] s);
    case (s)
{valid_arms}      default: slot_valid_f = 1'b0;
    endcase
  endfunction
  function [31:0] layer_off_f(input [3:0] n);
    case (n)
{layer_arms}      default: layer_off_f = 32'd0;
    endcase
  endfunction

  // Tags: 2-bit state (0 invalid, 1 filling, 2 ready) and 22-bit key.
  reg [1:0]  tstate [0:{last_line}];
  reg [21:0] tkey   [0:{last_line}];
  reg [2:0]  plru   [0:{last_set}];

  // Four explicit synchronous banks. Their contents are only read once a line
  // is READY, which requires a complete sixteen-beat refill of all 64 words.
  reg [15:0] bank0 [0:{last_word}];
  reg [15:0] bank1 [0:{last_word}];
  reg [15:0] bank2 [0:{last_word}];
  reg [15:0] bank3 [0:{last_word}];

  // One demand head, one captured result and one synchronous read issue.
  reg        head_valid;
  reg [71:0] head_payload;
  reg        res_valid;
  reg [71:0] res_payload;
  reg [15:0] res_tex [0:3];
  reg        rd_valid;
  reg [71:0] rd_payload;
  reg [5:0]  rd_line;
  reg [3:0]  rd_local [0:3];
  reg [1:0]  rd_tap   [0:3];
  reg        pin_valid;
  reg [5:0]  pin_line;

  // One demand descriptor, presented until the memory accepts it.
  reg        desc_valid;
  reg [21:0] desc_key;
  reg [5:0]  desc_line;
  reg [31:0] desc_addr;
  reg        desc_started;
  reg        desc_presented;
  reg [4:0]  desc_next;

  wire [21:0] head_key  = head_payload[21:0];
  wire [2:0]  head_tlx  = head_payload[24:22];
  wire [2:0]  head_tly  = head_payload[27:25];
  wire [3:0]  head_set  = {{head_key[16:15], head_key[9:8]}};

  // Head address: base + (prefix + y*side + x)*128.
  wire [3:0]  hk_slot = head_key[3:0];
  wire [3:0]  hk_n    = head_key[7:4];
  wire [6:0]  hk_x    = head_key[14:8];
  wire [6:0]  hk_y    = head_key[21:15];
  wire [31:0] hk_lay  = slot_full_f(hk_slot) ? layer_off_f(hk_n) : 32'd0;
  wire [31:0] hk_row = (hk_n < 4'd3) ? {{25'd0,hk_y}} : ({{25'd0,hk_y}} << (hk_n - 4'd3));
  wire [31:0] hk_addr = slot_base_f(hk_slot) + ((hk_lay + hk_row + hk_x) << 7);

  // Raw port inputs must satisfy the same immutable slot/level/tile contract
  // as TileKey::address. No malformed input can allocate or publish a line.
  wire [3:0] ik_slot = in_payload[3:0];
  wire [3:0] ik_n = in_payload[7:4];
  wire [7:0] ik_side = (ik_n < 4'd3) ? 8'd1 : (8'd1 << (ik_n - 4'd3));
  wire input_bad = ce && in_valid && !head_valid && !fault && !abort &&
      (!slot_valid_f(ik_slot) || ik_n > 4'd10 || ik_n > slot_max_f(ik_slot) ||
       (!slot_full_f(ik_slot) && ik_n != slot_max_f(ik_slot)) ||
       {{1'b0,in_payload[14:8]}} >= ik_side || {{1'b0,in_payload[21:15]}} >= ik_side);

  // Transport event separation. A request is offered while unstarted; a
  // presented request stays offered across fault/abort. Acceptance, beats and
  // the terminal are independent; a same-edge accept+first-beat is legal.
  wire        req_present = desc_valid && !desc_started && (!fault || desc_presented);
  wire        req_accept  = req_present && mem_req_ready;
  wire        eff_started = desc_valid && (desc_started || req_accept);
  wire        beat_present = mem_resp_valid;
  wire        beat_ok = beat_present && eff_started && (desc_next < 5'd16) &&
                        (mem_resp_index == desc_next[3:0]);
  wire [4:0]  eff_next = desc_next + (beat_ok ? 5'd1 : 5'd0);
  wire        terminal = mem_resp_complete;
  wire        term_ok = terminal && mem_resp_ok && eff_started && (eff_next == 5'd16);
  wire        proto_fault = (beat_present && !beat_ok) || (terminal && !term_ok);
  // Request validity must not depend on a response that may occur on this
  // very acceptance edge; doing so creates a combinational loop.
  wire        fault_latched = fault || abort;
  wire        fault_eff = fault_latched || proto_fault || input_bad;

  assign in_ready = ce && !head_valid && !fault_eff;
  assign out_valid = res_valid && !fault_eff;
  assign out_payload = res_payload;
  assign out_tex0 = res_tex[0];
  assign out_tex1 = res_tex[1];
  assign out_tex2 = res_tex[2];
  assign out_tex3 = res_tex[3];

  assign mem_req_valid = req_present && (!fault_latched || desc_presented);
  assign mem_req_addr  = desc_addr;
  assign drained = !desc_valid;

  // Lookup by the head key in its set, using only the pre-edge tag state.
  reg        hit_ready;
  reg [5:0]  hit_line;
  reg        look_filling;
  // Victim: an Invalid way first, otherwise the tree-PLRU Ready way.
  reg        vic_valid;
  reg [5:0]  vic_line;
  integer    wl, wv, wr;
  reg [1:0]  o0, o1, o2, o3;
  always @(*) begin
    hit_ready = 1'b0; hit_line = 6'd0;
    look_filling = 1'b0;
    for (wl = 0; wl < {ways}; wl = wl + 1) begin
      if ((tstate[head_set*{ways}+wl] != 2'd0) &&
          (tkey[head_set*{ways}+wl] == head_key)) begin
        if (tstate[head_set*{ways}+wl] == 2'd2) begin
          hit_ready = 1'b1; hit_line = head_set*{ways}+wl;
        end else begin
          look_filling = 1'b1;
        end
      end
    end
  end
  always @(*) begin
    vic_valid = 1'b0; vic_line = 6'd0;
    for (wv = 0; wv < {ways}; wv = wv + 1) begin
      if (!vic_valid && (tstate[head_set*{ways}+wv] == 2'd0) &&
          !(pin_valid && (pin_line == head_set*{ways}+wv))) begin
        vic_valid = 1'b1; vic_line = head_set*{ways}+wv;
      end
    end
    if (!vic_valid) begin
      case (plru[head_set])
        3'd0: begin o0=2'd0; o1=2'd1; o2=2'd2; o3=2'd3; end
        3'd1: begin o0=2'd2; o1=2'd3; o2=2'd0; o3=2'd1; end
        3'd2: begin o0=2'd1; o1=2'd0; o2=2'd2; o3=2'd3; end
        3'd3: begin o0=2'd2; o1=2'd3; o2=2'd1; o3=2'd0; end
        3'd4: begin o0=2'd0; o1=2'd1; o2=2'd3; o3=2'd2; end
        3'd5: begin o0=2'd3; o1=2'd2; o2=2'd0; o3=2'd1; end
        3'd6: begin o0=2'd1; o1=2'd0; o2=2'd3; o3=2'd2; end
        default: begin o0=2'd3; o1=2'd2; o2=2'd1; o3=2'd0; end
      endcase
      if (tstate[head_set*{ways}+o0] == 2'd2 &&
          !(pin_valid && (pin_line == head_set*{ways}+o0))) begin
        vic_valid = 1'b1; vic_line = head_set*{ways}+o0;
      end else if (tstate[head_set*{ways}+o1] == 2'd2 &&
          !(pin_valid && (pin_line == head_set*{ways}+o1))) begin
        vic_valid = 1'b1; vic_line = head_set*{ways}+o1;
      end else if (tstate[head_set*{ways}+o2] == 2'd2 &&
          !(pin_valid && (pin_line == head_set*{ways}+o2))) begin
        vic_valid = 1'b1; vic_line = head_set*{ways}+o2;
      end else if (tstate[head_set*{ways}+o3] == 2'd2 &&
          !(pin_valid && (pin_line == head_set*{ways}+o3))) begin
        vic_valid = 1'b1; vic_line = head_set*{ways}+o3;
      end
    end
  end

  // Tap routing for the head's 2x2 local footprint (wrapped 7->0).
  integer t;
  reg [3:0] tx, ty;
  reg [1:0] tbank;
  reg [3:0] tlocal;
  reg [3:0] cnt;   // beat loop
  reg [2:0] bx;
  reg [2:0] by;
  reg [1:0] bsel;
  reg [3:0] blocal;
  reg [3:0] tset;

  always @(posedge clk) begin
    if (reset) begin
      head_valid <= 1'b0; res_valid <= 1'b0; rd_valid <= 1'b0;
      pin_valid <= 1'b0; desc_valid <= 1'b0; desc_started <= 1'b0;
      desc_presented <= 1'b0; desc_next <= 5'd0; fault <= 1'b0;
      for (wr = 0; wr < {lines}; wr = wr + 1) begin
        tstate[wr] <= 2'd0;
      end
      for (wr = 0; wr < {sets}; wr = wr + 1) begin
        plru[wr] <= 3'd0;
      end
    end else begin
      if (fault_eff) fault <= 1'b1;

      // ---- Accepted maintenance: independent of the compute CE. ----
      if (mem_req_valid && mem_req_ready) begin
        desc_started <= 1'b1;
        desc_presented <= 1'b1;
      end
      if (mem_req_valid) begin
        desc_presented <= 1'b1;
      end

      if (beat_present) begin
        if (fault_eff) begin
          // Draining a faulted/aborted burst: discard the number, keep order.
          if (desc_valid && eff_started && (desc_next < 5'd16) &&
              (mem_resp_index == desc_next[3:0])) begin
            desc_next <= eff_next;
          end
        end else if (beat_ok) begin
          for (cnt = 0; cnt < 4; cnt = cnt + 1) begin
            bx = (mem_resp_index * 4 + cnt) % 8;
            by = (mem_resp_index * 4 + cnt) / 8;
            bsel = {{by[0] ^ bx[1], bx[0]}};
            blocal = {{by[2:0], bx[2]}};
            case (bsel)
              2'd0: bank0[desc_line*16+blocal] <= mem_resp_data[cnt*16 +: 16];
              2'd1: bank1[desc_line*16+blocal] <= mem_resp_data[cnt*16 +: 16];
              2'd2: bank2[desc_line*16+blocal] <= mem_resp_data[cnt*16 +: 16];
              default: bank3[desc_line*16+blocal] <= mem_resp_data[cnt*16 +: 16];
            endcase
          end
          desc_next <= eff_next;
        end else begin
          fault <= 1'b1;
          if (desc_valid) tstate[desc_line] <= 2'd0;
        end
      end

      if (terminal) begin
        if (fault_eff) begin
          // A faulted/aborted terminal ends the transport; never publish.
          if (desc_valid) tstate[desc_line] <= 2'd0;
          desc_valid <= 1'b0;
        end else if (term_ok) begin
          tstate[desc_line] <= 2'd2;
          tkey[desc_line] <= desc_key;
          tset = desc_line[5:2];
          if (desc_line[1] == 1'b0)
            plru[tset] <= {{plru[tset][2], (desc_line[0] ^ 1'b1), 1'b1}};
          else
            plru[tset] <= {{(desc_line[0] ^ 1'b1), plru[tset][1], 1'b0}};
          desc_valid <= 1'b0;
        end else begin
          fault <= 1'b1;
          if (desc_valid) tstate[desc_line] <= 2'd0;
          desc_valid <= 1'b0;
        end
      end

      // ---- Explicit abort: suppress compute, keep a presented request. ----
      if (fault_eff) begin
        head_valid <= 1'b0; res_valid <= 1'b0; rd_valid <= 1'b0;
        pin_valid <= 1'b0;
        if (desc_valid) begin
          tstate[desc_line] <= 2'd0;
          if (!desc_presented && !mem_req_valid) desc_valid <= 1'b0;
        end
      end

      // ---- Compute: every register below is gated by the enabled edge. ----
      if (ce && !fault_eff) begin
        // 1. Output transfer from the pre-edge result slot only.
        if (out_ready && res_valid) begin
          res_valid <= 1'b0;
        end

        // 2. Capture the pending synchronous RAM read if the result slot was
        //    free before this edge.
        if (rd_valid && !res_valid) begin
          res_tex[rd_tap[0]] <= bank0[rd_line*16+rd_local[0]];
          res_tex[rd_tap[1]] <= bank1[rd_line*16+rd_local[1]];
          res_tex[rd_tap[2]] <= bank2[rd_line*16+rd_local[2]];
          res_tex[rd_tap[3]] <= bank3[rd_line*16+rd_local[3]];
          res_payload <= rd_payload;
          res_valid <= 1'b1;
          rd_valid <= 1'b0;
          pin_valid <= 1'b0;
        end

        // 3. Issue a synchronous read for a READY-hit head. The result is free
        //    for a next-edge capture when it is empty or leaving this edge.
        if (!rd_valid && (!res_valid || out_ready) && head_valid && hit_ready) begin
          for (t = 0; t < 4; t = t + 1) begin
            tx = (head_tlx + t[0]) & 4'h7;
            ty = (head_tly + t[1]) & 4'h7;
            tbank = {{ty[0] ^ tx[1], tx[0]}};
            tlocal = {{ty[2:0], tx[2]}};
            rd_local[tbank] <= tlocal;
            rd_tap[tbank] <= t[1:0];
          end
          rd_payload <= head_payload;
          rd_line <= hit_line;
          rd_valid <= 1'b1;
          pin_valid <= 1'b1;
          pin_line <= hit_line;
          head_valid <= 1'b0;
          tset = hit_line[5:2];
          if (hit_line[1] == 1'b0)
            plru[tset] <= {{plru[tset][2], (hit_line[0] ^ 1'b1), 1'b1}};
          else
            plru[tset] <= {{(hit_line[0] ^ 1'b1), plru[tset][1], 1'b0}};
        end

        // 4. Allocate the single demand descriptor on a genuine miss.
        if (!desc_valid && head_valid && !hit_ready && !look_filling && vic_valid) begin
          tstate[vic_line] <= 2'd1;
          tkey[vic_line] <= head_key;
          desc_key <= head_key;
          desc_line <= vic_line;
          desc_addr <= hk_addr;
          desc_started <= 1'b0;
          desc_presented <= 1'b0;
          desc_next <= 5'd0;
          desc_valid <= 1'b1;
        end

        // 5. Admit a new input only when the head was free before this edge.
        if (in_valid && in_ready) begin
          head_valid <= 1'b1;
          head_payload <= in_payload;
        end
      end
    end
  end
endmodule
"#,
        last_line = LINES - 1,
        last_set = SETS - 1,
        last_word = BANK_DEPTH - 1,
        ways = WAYS,
        lines = LINES,
        sets = SETS,
    )
}

/// Emit the complete synthesizable module for the given immutable slot table.
pub fn verilog(slots: &[Slot]) -> String {
    mut_verilog(slots)
}

/// Alias matching the `source()` convention of the other hand-written leaves.
pub fn source(slots: &[Slot]) -> String {
    verilog(slots)
}

/// Refill beat count, re-exported for the test harness.
pub fn refill_beats() -> usize {
    REFILL_BEATS
}

/// Bank count, re-exported for the test harness.
pub fn banks() -> usize {
    BANKS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_declares_ports_and_is_slot_specialized() {
        let slots = vec![Slot {
            base_address: 0x1000,
            has_full_mip: true,
            max_size_log2: 10,
            valid: true,
        }];
        let source = verilog(&slots);
        assert!(source.contains(&format!("module {MODULE}(")));
        assert!(source.contains("input  wire        abort,"));
        assert!(source.contains("slot_base_f = 32'h00001000;"));
        assert!(source.contains("layer_off_f = 32'd5464;"));
        assert!(source.contains("reg [15:0] bank3 [0:1023];"));
        assert!(source.contains("desc_presented"));
        assert!(!source.contains("fill_hit"));
        assert_eq!(allocation().bank_bits, 4 * 1024 * 16);
        assert_eq!(allocation().read_bits, 103);
        assert_eq!(allocation().descriptor_bits, 68);
    }
}
