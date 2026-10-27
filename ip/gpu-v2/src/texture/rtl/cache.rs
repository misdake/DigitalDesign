//! Generator for the demand-only texture-cache leaf RTL.
//!
//! The emitted Verilog is a fixed-width, slot-specialized mirror of
//! [`crate::texture::emu::cache::CacheEmu`]. It is produced from the immutable
//! [`Slot`] table so the address base/level ROM is literal wiring. The Gowin
//! backend uses two masked-write tag BSRAMs, three set-state/PLRU SRAM cells
//! and four synchronous data BSRAMs. Admission reads all four tags before the
//! first lookup. Reset masks hide stale state; allocation writes the key and
//! the terminal publishes READY. The ignored Icarus tests qualify both physical
//! and portable backends edge for edge against the independent emulator.
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

use crate::texture::emu::cache::{Allocation, BANKS, BANK_DEPTH, REFILL_BEATS, SETS, WAYS};
use crate::texture::ports::{layer_offset, Slot};

/// Verilog module name emitted by [`verilog`].
pub const MODULE: &str = "gpu_v2_texture_cache";

/// Logical reference capacity shared with the emu, not the RTL's fitted FF count.
/// The physical backend compresses tag keys, moves set rows into SRAM and
/// derives tap permutation from the held payload instead of retaining it.
pub fn allocation() -> Allocation {
    Allocation::default()
}

fn slot_base_literal(slot: Option<&Slot>) -> String {
    format!(
        "32'h{:08x}",
        slot.filter(|s| s.valid).map_or(0, |s| s.base_address)
    )
}

fn mut_verilog(slots: &[Slot], strategy: HitDecode, bank_ordered: bool) -> String {
    let output_map = if bank_ordered {
        r#"  // Private Serial cache/Color boundary: weights follow physical banks.
  // Inverse-permute36 weight bits instead of permuting64 texel bits.
  wire a=res_payload[22];
  wire y0=res_payload[25]^res_payload[23],y1=y0^a;
  wire [8:0] w0=res_payload[36:28],w1=res_payload[45:37];
  wire [8:0] w2=res_payload[54:46],w3=res_payload[63:55];
  wire [8:0] z0=y0?w2:w0,z2=y0?w0:w2;
  wire [8:0] z1=y1?w3:w1,z3=y1?w1:w3;
  wire [8:0] b0=a?z1:z0,b1=a?z0:z1;
  wire [8:0] b2=a?z3:z2,b3=a?z2:z3;
  assign out_payload={res_payload[71:64],b3,b2,b1,b0,res_payload[27:0]};
  assign out_tex0=res_tex[0];assign out_tex1=res_tex[1];
  assign out_tex2=res_tex[2];assign out_tex3=res_tex[3];
"#
    } else {
        r#"  assign out_payload = res_payload;
  // Two conditional swaps implement the bank bijection; no dynamic write
  // decoder or four independent destination selectors are needed.
  wire [15:0] x0=res_payload[22]?res_tex[1]:res_tex[0];
  wire [15:0] x1=res_payload[22]?res_tex[0]:res_tex[1];
  wire [15:0] x2=res_payload[22]?res_tex[3]:res_tex[2];
  wire [15:0] x3=res_payload[22]?res_tex[2]:res_tex[3];
  wire y0=res_payload[25]^res_payload[23];
  wire y1=y0^res_payload[22];
  assign out_tex0=y0?x2:x0;
  assign out_tex1=y1?x3:x1;
  assign out_tex2=y0?x0:x2;
  assign out_tex3=y1?x1:x3;
"#
    };
    let hit_decode = match strategy {
        HitDecode::Fixed => {
            r#"  wire [3:0] ready_hits;
  wire [3:0] filling_hits;
  genvar hw;
  generate for (hw=0;hw<4;hw=hw+1) begin: fixed_hit_decode
    assign ready_hits[hw] = head_state[hw*2+:2] == 2'd2 && tag_q[hw] == head_tag;
    assign filling_hits[hw] = head_state[hw*2+:2] != 2'd0 && head_state[hw*2+:2] != 2'd2 && tag_q[hw] == head_tag;
  end endgenerate
  always @(*) begin
    hit_ready = |ready_hits;
    look_filling = |filling_hits;
    // Preserve the original highest-way priority with two fixed Boolean decodes.
    hit_line = {head_set, ready_hits[3] | ready_hits[2], ready_hits[3] | (ready_hits[1] & ~ready_hits[2])};
  end
"#
        }
        HitDecode::SerialPriority => {
            r#"  always @(*) begin
    hit_ready = 1'b0; hit_line = 6'd0;
    look_filling = 1'b0;
    for (wl = 0; wl < 4; wl = wl + 1) begin
      if ((head_state[wl*2+:2] != 2'd0) &&
          (tag_q[wl] == head_tag)) begin
        if (head_state[wl*2+:2] == 2'd2) begin
          hit_ready = 1'b1; hit_line = head_set*4+wl;
        end else begin
          look_filling = 1'b1;
        end
      end
    end
  end
"#
        }
    };
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

  // Reset masks hide stale SRAM rows without clearing RAM contents.
  (* syn_ramstyle="distributed_ram" *) reg[7:0] state_rows[0:{last_set}];
  (* syn_ramstyle="distributed_ram" *) reg[2:0] plru_rows[0:{last_set}];
  reg [15:0] state_initialized;
  // Four set bits are carried by the address; each retained tag needs 18 bits.
  wire[17:0] tag_q[0:3];
  reg [15:0] plru_initialized;

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
  // Tap permutation is derived from the held result coordinates.
  reg        pin_valid;
  reg [5:0]  pin_line;

  // One demand descriptor, presented until the memory accepts it.
  reg        desc_valid;
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
  // Valid x occupies the bits below y; concatenate the tile index.
  wire [13:0] hk_row = {{7'd0,hk_y}} << ((hk_n < 4'd3)?4'd0:(hk_n-4'd3));
  wire [13:0] hk_tile = hk_row | {{7'd0,hk_x}};
  wire [14:0] hk_offset = {{1'b0,hk_tile}}+{{1'b0,hk_lay[13:0]}};
  wire [31:0] hk_addr = slot_base_f(hk_slot)+{{10'd0,hk_offset,7'd0}};

  // Raw port inputs must satisfy the same immutable slot/level/tile contract
  // as TileKey::address. No malformed input can allocate or publish a line.
  wire [3:0] ik_slot = in_payload[3:0];
  wire [3:0] ik_n = in_payload[7:4];
  // Both axes use the same power-of-two bound. Share the high-bit check.
  wire [6:0] ik_xy = in_payload[14:8] | in_payload[21:15];
  wire ik_coord_bad = (ik_xy[0] && ik_n < 4'd4) ||
      (ik_xy[1] && ik_n < 4'd5) || (ik_xy[2] && ik_n < 4'd6) ||
      (ik_xy[3] && ik_n < 4'd7) || (ik_xy[4] && ik_n < 4'd8) ||
      (ik_xy[5] && ik_n < 4'd9) || (ik_xy[6] && ik_n < 4'd10);
  wire input_bad = ce && in_valid && !head_valid && !fault && !abort &&
      (!slot_valid_f(ik_slot) || ik_n > 4'd10 || ik_n > slot_max_f(ik_slot) ||
       (!slot_full_f(ik_slot) && ik_n != slot_max_f(ik_slot)) ||
       ik_coord_bad);

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
{output_map}

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
  integer    wl, wv;
  reg [1:0]  o0, o1;

  wire[17:0] head_tag={{head_key[21:17],head_key[14:10],head_key[7:0]}};
  wire tag_write=!reset && ce && !fault_eff && !desc_valid && head_valid &&
      !hit_ready && !look_filling && vic_valid;
  wire tag_read=!reset && !fault_eff && !tag_write && (head_valid || (in_valid && in_ready));
  wire[3:0] tag_set=head_valid?head_set:{{in_payload[16:15],in_payload[9:8]}};
  // A cache head remains unchanged from allocation until its terminal/read.
  // Admission prefetch supplies the first lookup without another enabled edge.
  // Allocation gates all tag reads; the held head refreshes on following clocks.
`ifdef GPU_V2_GOWIN_DSP
  genvar tg;
  generate for(tg=0;tg<2;tg=tg+1)begin:tag_pair
    wire[35:0] pair_q;
    // GW2AR writes one complete tag; the read port returns both adjacent tags.
    SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(18),.BIT_WIDTH_1(36)) bank(
      .DO(pair_q),.DI({{18'd0,head_tag}}),
      .BLKSELA(3'b000),.BLKSELB(3'b000),
      .ADA({{5'd0,vic_line[5:2],vic_line[0],4'b0011}}),
      .ADB({{5'd0,tag_set,5'd0}}),.CLKA(clk),.CLKB(clk),
      .CEA(tag_write && vic_line[1]==tg),.CEB(tag_read),
      .OCE(1'b1),.RESETA(1'b0),.RESETB(1'b0));
    assign tag_q[tg*2]=pair_q[17:0];assign tag_q[tg*2+1]=pair_q[35:18];
  end endgenerate
`else
  reg[17:0] tag_ram[0:63];reg[17:0] tag_read_q[0:3];integer tr;
  always @(posedge clk)begin
    if(tag_write)tag_ram[vic_line]<=head_tag;
    if(tag_read)for(tr=0;tr<4;tr=tr+1)tag_read_q[tr]<=tag_ram[{{tag_set,2'b00}}+tr];
  end
  genvar tp;generate for(tp=0;tp<4;tp=tp+1)begin:tag_port
    assign tag_q[tp]=tag_read_q[tp];
  end endgenerate
`endif
  wire[7:0] head_state=state_initialized[head_set]?state_rows[head_set]:8'd0;
  wire[2:0] head_plru=plru_initialized[head_set]?plru_rows[head_set]:3'd0;
  wire read_issue=ce && !fault_eff && !rd_valid && (!res_valid || out_ready) && head_valid && hit_ready;
  wire publish=terminal && !fault_eff && term_ok;
  wire state_write=tag_write || (desc_valid && (terminal || fault_eff || (beat_present && !beat_ok)));
  wire[1:0] state_way=tag_write?vic_line[1:0]:desc_line[1:0];
  wire[1:0] state_value=tag_write?2'd1:(publish?2'd2:2'd0);
  wire[7:0] state_next;
  genvar sw;
  generate for(sw=0;sw<4;sw=sw+1)begin:state_update
    assign state_next[sw*2+:2]=(state_way==sw)?state_value:head_state[sw*2+:2];
  end endgenerate
  wire plru_write=publish || read_issue;
  wire[1:0] used_way=publish?desc_line[1:0]:hit_line[1:0];
  wire[2:0] plru_next=used_way[1]?{{(used_way[0]^1'b1),head_plru[1],1'b0}}:
      {{head_plru[2],(used_way[0]^1'b1),1'b1}};
  // Both descriptor and hit keep the same head set until read issue. One port
  // suffices: refill publication and a READY head issue are mutually exclusive.
  always @(posedge clk)begin
    if(!reset && state_write)state_rows[head_set]<=state_next;
    if(!reset && plru_write)plru_rows[head_set]<=plru_next;
  end
  always @(posedge clk)begin
    if(reset)begin state_initialized<=0;plru_initialized<=0;end
    else begin
      if(state_write)state_initialized[head_set]<=1;
      if(plru_write)plru_initialized[head_set]<=1;
    end
  end

{hit_decode}  always @(*) begin
    vic_valid = 1'b0; vic_line = 6'd0;
    for (wv = 0; wv < {ways}; wv = wv + 1) begin
      if (!vic_valid && (head_state[wv*2+:2] == 2'd0) &&
          !(pin_valid && (pin_line == head_set*4+wv))) begin
        vic_valid = 1'b1; vic_line = head_set*4+wv;
      end
    end
    if (!vic_valid) begin
      // No descriptor means every non-invalid line is READY. Only one
      // line may be pinned, so the PLRU first choice or its sibling suffices.
      o0={{head_plru[0],(head_plru[0]?head_plru[2]:head_plru[1])}};
      o1=o0^2'd1;
      vic_valid=1'b1;
      if(pin_valid && pin_line=={{head_set,o0}})vic_line={{head_set,o1}};
      else vic_line={{head_set,o0}};
    end
  end

  // Inverse tap map for each fixed bank, including7->0 repeat wrap.
  wire[1:0] bank_tap0={{(1'b0 ^ head_tly[0] ^ head_tlx[1] ^ (head_tlx[0] & (1'b0 ^ head_tlx[0]))),(1'b0 ^ head_tlx[0])}};
  wire[2:0] bank_x0=head_tlx+bank_tap0[0],bank_y0=head_tly+bank_tap0[1];
  wire[1:0] bank_tap1={{(1'b0 ^ head_tly[0] ^ head_tlx[1] ^ (head_tlx[0] & (1'b1 ^ head_tlx[0]))),(1'b1 ^ head_tlx[0])}};
  wire[2:0] bank_x1=head_tlx+bank_tap1[0],bank_y1=head_tly+bank_tap1[1];
  wire[1:0] bank_tap2={{(1'b1 ^ head_tly[0] ^ head_tlx[1] ^ (head_tlx[0] & (1'b0 ^ head_tlx[0]))),(1'b0 ^ head_tlx[0])}};
  wire[2:0] bank_x2=head_tlx+bank_tap2[0],bank_y2=head_tly+bank_tap2[1];
  wire[1:0] bank_tap3={{(1'b1 ^ head_tly[0] ^ head_tlx[1] ^ (head_tlx[0] & (1'b1 ^ head_tlx[0]))),(1'b1 ^ head_tlx[0])}};
  wire[2:0] bank_x3=head_tlx+bank_tap3[0],bank_y3=head_tly+bank_tap3[1];

  always @(posedge clk) begin
    if (reset) begin
      head_valid <= 1'b0; res_valid <= 1'b0; rd_valid <= 1'b0;
      pin_valid <= 1'b0; desc_valid <= 1'b0; desc_started <= 1'b0;
      desc_presented <= 1'b0; desc_next <= 5'd0; fault <= 1'b0;
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
          // Four words share one bank-local address; index bit1 swaps word pairs.
          bank0[{{desc_line,mem_resp_index[3:1],mem_resp_index[0]}}] <= mem_resp_index[1] ? mem_resp_data[32+:16] : mem_resp_data[0+:16];
          bank1[{{desc_line,mem_resp_index[3:1],mem_resp_index[0]}}] <= mem_resp_index[1] ? mem_resp_data[48+:16] : mem_resp_data[16+:16];
          bank2[{{desc_line,mem_resp_index[3:1],mem_resp_index[0]}}] <= mem_resp_index[1] ? mem_resp_data[0+:16] : mem_resp_data[32+:16];
          bank3[{{desc_line,mem_resp_index[3:1],mem_resp_index[0]}}] <= mem_resp_index[1] ? mem_resp_data[16+:16] : mem_resp_data[48+:16];
          desc_next <= eff_next;
        end else begin
          fault <= 1'b1;
          // State row port invalidates the demand way.
        end
      end

      if (terminal) begin
        if (fault_eff) begin
          // A faulted/aborted terminal ends the transport; never publish.
          // State row port invalidates the demand way.
          desc_valid <= 1'b0;
        end else if (term_ok) begin

          // Key was written at allocation; ACK publishes state only.
          desc_valid <= 1'b0;
        end else begin
          fault <= 1'b1;
          // State row port invalidates the demand way.
          desc_valid <= 1'b0;
        end
      end

      // ---- Explicit abort: suppress compute, keep a presented request. ----
      if (fault_eff) begin
        head_valid <= 1'b0; res_valid <= 1'b0; rd_valid <= 1'b0;
        pin_valid <= 1'b0;
        if (desc_valid) begin

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
          res_tex[0] <= bank0[rd_line*16+rd_local[0]];
          res_tex[1] <= bank1[rd_line*16+rd_local[1]];
          res_tex[2] <= bank2[rd_line*16+rd_local[2]];
          res_tex[3] <= bank3[rd_line*16+rd_local[3]];
          res_payload <= rd_payload;
          res_valid <= 1'b1;
          rd_valid <= 1'b0;
          pin_valid <= 1'b0;
        end

        // 3. Issue a synchronous read for a READY-hit head. The result is free
        //    for a next-edge capture when it is empty or leaving this edge.
        if (!rd_valid && (!res_valid || out_ready) && head_valid && hit_ready) begin
          rd_local[0] <= {{bank_y0,bank_x0[2]}};
          rd_local[1] <= {{bank_y1,bank_x1[2]}};
          rd_local[2] <= {{bank_y2,bank_x2[2]}};
          rd_local[3] <= {{bank_y3,bank_x3[2]}};
          rd_payload <= head_payload;
          rd_line <= hit_line;
          rd_valid <= 1'b1;
          pin_valid <= 1'b1;
          pin_line <= hit_line;
          head_valid <= 1'b0;

        end

        // 4. Allocate the single demand descriptor on a genuine miss.
        if (!desc_valid && head_valid && !hit_ready && !look_filling && vic_valid) begin

          // Single tag write is performed by the fixed bank port below.
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
        last_set = SETS - 1,
        last_word = BANK_DEPTH - 1,
        ways = WAYS,
    )
}

/// Emit the complete synthesizable module for the given immutable slot table.
pub fn verilog(slots: &[Slot]) -> String {
    verilog_with_hit_decode(slots, HitDecode::Fixed)
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
    fn fixed_hit_decode_preserves_highest_ready_way_and_filling() {
        assert_eq!(WAYS, 4);
        let source = verilog(&[]);
        assert!(source.contains("ready_hits[3] | (ready_hits[1] & ~ready_hits[2])"));
        assert!(source.contains("assign filling_hits[hw] = head_state[hw*2+:2] != 2'd0"));
        for set in 0_u8..16 {
            for states in 0_u16..256 {
                for matching in 0_u8..16 {
                    let mut expected = None;
                    let mut filling = false;
                    let mut ready = 0u8;
                    for way in 0..4 {
                        let state = (states >> (2 * way)) & 3;
                        if matching >> way & 1 != 0 && state != 0 {
                            if state == 2 {
                                expected = Some(set * 4 + way);
                                ready |= 1 << way;
                            } else {
                                filling = true;
                            }
                        }
                    }
                    let encoded = (set << 2)
                        | (((ready >> 3 | ready >> 2) & 1) << 1)
                        | ((ready >> 3 | (ready >> 1 & !(ready >> 2))) & 1);
                    assert_eq!(ready != 0, expected.is_some());
                    if let Some(line) = expected {
                        assert_eq!(encoded, line);
                    }
                    let actual_filling = (0..4).any(|way| {
                        matching >> way & 1 != 0 && !matches!((states >> (2 * way)) & 3, 0 | 2)
                    });
                    assert_eq!(filling, actual_filling);
                }
            }
        }
    }

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

/// Explicit emitter strategy; both preserve the highest matching READY way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitDecode {
    Fixed,
    SerialPriority,
}
pub fn verilog_with_hit_decode(slots: &[Slot], strategy: HitDecode) -> String {
    mut_verilog(slots, strategy, false)
}

/// Private Serial composition only. Output texels stay in physical bank order
/// and the four payload weights are inverse-permuted to match. Key, local
/// coordinates and first/last remain unchanged. The public cache API retains
/// canonical tap order and an unchanged Group4 payload.
pub(crate) fn verilog_serial_bank_ordered(slots: &[Slot]) -> String {
    mut_verilog(slots, HitDecode::SerialPriority, true)
}
