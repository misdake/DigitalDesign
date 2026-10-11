//! Fixed two-phase Final/ROP lowering for the closed immutable-draw chain.
//! Final keeps eight integer bits per specular term; Sampling remains Q8.1.
//! Shared MULT9X9 outputs have one enabled-edge latency. Raw validation remains
//! independent of the common commit enable; paid memory responses keep draining.
//! Source anchors specialize the existing qualified SH9 factory, while its
//! generic standalone components and the committed selected factory remain usable.

pub fn lower(source: String, native: bool) -> Result<String, String> {
    let mut source = source.replace("\r\n", "\n");
    for &(old, new) in COMMON.iter().chain(if native { NATIVE } else { PORTABLE }) {
        if source.matches(old).count() != 1 {
            return Err(format!(
                "Final/ROP cadence anchor: {}",
                old.lines().next().unwrap_or("")
            ));
        }
        source = source.replacen(old, new, 1);
    }
    Ok(source)
}

const COMMON: &[(&str, &str)] = &[
    (
        r###" input wire mem_write_ready,mem_complete_valid,mem_complete_ok
);
 wire front_fault,front_bad,advance,public_credit,cache_drained;wire raw_retire_proposal;
 wire sink_fault,sink_idle,final_fault,sink_bad,final_bad;
 wire coalition_allow=!(front_bad||sink_bad||final_bad);
"###,
        r###" input wire mem_write_ready,mem_complete_valid,mem_complete_ok
);
 wire [53:0] shared_rop_product;wire shared_rop_offer,shared_rop_ce;wire [26:0] shared_rop_a;wire[7:0]shared_rop_b;
 wire [50:0] shared_spec_product;wire shared_spec_ce;wire[8:0]shared_spec_a;wire[23:0]shared_spec_b;
 gpu_v2_final_rop_bank shared_bank(.clk(clk),.reset(reset),.rop_ce(shared_rop_ce),.rop_a(shared_rop_a),.rop_b(shared_rop_b),.rop_product(shared_rop_product),
 .spec_ce(shared_spec_ce),.spec_a(shared_spec_a),.spec_b(shared_spec_b),.spec_product(shared_spec_product));
 wire front_fault,front_bad,advance,public_credit,cache_drained;
 reg shade_phase;
 always @(posedge clk)if(reset)shade_phase<=0;else if(advance)shade_phase<=!shade_phase;wire raw_retire_proposal;
 wire sink_fault,sink_idle,final_fault,sink_bad,final_bad;
 wire coalition_allow=!(front_bad||sink_bad||final_bad);
"###,
    ),
    (
        r###" wire final_out_ready=lane_valid&&sink_color_ready&&out_covered;
 gpu_v2_postquad_final final_color(
 .clk(clk),.reset(reset),.ce(advance),.proposal_ce(compute_ce),.commit_allow(coalition_allow),.preflight_bad(final_bad),.abort(abort||stopped||sink_fault),
 .in_valid(final_in_valid),.in_ready(final_in_ready),
"###,
        r###" wire final_out_ready=lane_valid&&sink_color_ready&&out_covered;
 gpu_v2_postquad_final final_color(
 .shared_rop_offer(shared_rop_offer),.shared_spec_product(shared_spec_product),.shared_spec_ce(shared_spec_ce),.shared_spec_a(shared_spec_a),.shared_spec_b(shared_spec_b),
 .shade_phase(shade_phase),
 .clk(clk),.reset(reset),.ce(advance),.proposal_ce(compute_ce),.commit_allow(coalition_allow),.preflight_bad(final_bad),.abort(abort||stopped||sink_fault),
 .in_valid(final_in_valid),.in_ready(final_in_ready),
"###,
    ),
    (
        r###" wire fbreq,fbwrite,fbready,fbbeat,fbcomplete,fbwvalid,fbwready;wire[31:0]fbaddr;wire[63:0]fbwdata;
 gpu_v2_pixel_framebuffer_sink sink(
 .clk(clk),.reset(reset),.ce(compute_ce),.commit_allow(coalition_allow),.preflight_bad(sink_bad),.lane_valid(lane_valid),.lane(out_lane),
 .rgba({alpha,out_covered?final_RGB:24'd0}),.depth(depth_rows[out_lane]),
"###,
        r###" wire fbreq,fbwrite,fbready,fbbeat,fbcomplete,fbwvalid,fbwready;wire[31:0]fbaddr;wire[63:0]fbwdata;
 gpu_v2_pixel_framebuffer_sink sink(
 .shared_rop_product(shared_rop_product),.shared_rop_offer(shared_rop_offer),.shared_rop_ce(shared_rop_ce),.shared_rop_a(shared_rop_a),.shared_rop_b(shared_rop_b),
 .shade_phase(shade_phase),
 .clk(clk),.reset(reset),.ce(compute_ce),.commit_allow(coalition_allow),.preflight_bad(sink_bad),.lane_valid(lane_valid),.lane(out_lane),
 .rgba({alpha,out_covered?final_RGB:24'd0}),.depth(depth_rows[out_lane]),
"###,
    ),
    (
        r###" parameter [8:0] HEIGHT=9'd256
)(
 input wire clk,reset,ce,commit_allow,
 output wire preflight_bad,
"###,
        r###" parameter [8:0] HEIGHT=9'd256
)(
 input wire shade_phase,
 input wire [53:0] shared_rop_product, output wire shared_rop_offer,shared_rop_ce, output wire [26:0] shared_rop_a, output wire [7:0] shared_rop_b,
 input wire clk,reset,ce,commit_allow,
 output wire preflight_bad,
"###,
    ),
    (
        r###" assign lane_accept=row_accept&&phase&&lane_matches;
 assign color_accept=row_accept&&!phase&&lane_matches;
 assign color_ready=!reset&&!phase&&lane_matches&&row_ready;
 assign quad_accepted=lane_accept&&(expected_lane==2'd3);
 wire flush_boundary=!lane_valid&&!phase&&(expected_lane==2'd0);
"###,
        r###" assign lane_accept=row_accept&&phase&&lane_matches;
 assign color_accept=row_accept&&!phase&&lane_matches;
 assign color_ready=!shade_phase&&!reset&&!phase&&lane_matches&&row_ready;
 assign quad_accepted=lane_accept&&(expected_lane==2'd3);
 wire flush_boundary=!lane_valid&&!phase&&(expected_lane==2'd0);
"###,
    ),
    (
        r###" framebuffer_cache #(.COLOR_BASE(COLOR_BASE),.DEPTH_BASE(DEPTH_BASE),
  .WIDTH(WIDTH),.HEIGHT(HEIGHT)) cache(
  .clk(clk),.reset(reset),.ce(ce&&!reset),.commit_allow(commit_allow),.preflight_bad(preflight_bad),
  .row_valid(lane_valid&&!reset),.row_x(x),.row_y(y),.row_mask(mask),
  .row_index(row_index),.row_data(row_data),.row_accept(row_accept),.row_ready(row_ready),
  .depth_func(depth_func),.depth_write(depth_write),.blend(blend),
"###,
        r###" framebuffer_cache #(.COLOR_BASE(COLOR_BASE),.DEPTH_BASE(DEPTH_BASE),
  .WIDTH(WIDTH),.HEIGHT(HEIGHT)) cache(
  .shared_rop_product(shared_rop_product),.shared_rop_offer(shared_rop_offer),.shared_rop_ce(shared_rop_ce),.shared_rop_a(shared_rop_a),.shared_rop_b(shared_rop_b),
  .shade_phase(shade_phase),.clk(clk),.reset(reset),.ce(ce&&!reset),.commit_allow(commit_allow),.preflight_bad(preflight_bad),
  .row_valid(lane_valid&&!reset&&(phase==shade_phase)),.row_x(x),.row_y(y),.row_mask(mask),
  .row_index(row_index),.row_data(row_data),.row_accept(row_accept),.row_ready(row_ready),
  .depth_func(depth_func),.depth_write(depth_write),.blend(blend),
"###,
    ),
    (
        r###"    parameter [8:0]  HEIGHT     = 9'd16
)(
    input             clk,
    input             reset,
"###,
        r###"    parameter [8:0]  HEIGHT     = 9'd16
)(
    input wire [53:0] shared_rop_product, output wire shared_rop_offer,shared_rop_ce, output wire [26:0] shared_rop_a, output wire [7:0] shared_rop_b,
    input wire shade_phase,
    input             clk,
    input             reset,
"###,
    ),
    (
        r###"    wire        leaf_in_ready;
    wire        leaf_out_valid;
    wire [1:0]  leaf_out_key;
    wire [1:0] result_bank = leaf_out_key ^ {s_hx[head][1],1'b0};
    wire [15:0] leaf_new_color;
"###,
        r###"    wire        leaf_in_ready;
    wire        leaf_out_valid;
    // Single ordered return cursor is the lane authority.
    wire [1:0] leaf_out_key=return_key;
    wire [1:0] result_bank = leaf_out_key ^ {s_hx[head][1],1'b0};
    wire [15:0] leaf_new_color;
"###,
    ),
    (
        r###"    wire        leaf_cw;
    wire        leaf_dw;
    rop_leaf leaf (
        .clk(clk),
        .reset(reset || fault),
"###,
        r###"    wire        leaf_cw;
    wire        leaf_dw;
    wire shared_mul_pending;
    rop_leaf leaf (.shade_phase(shade_phase),
        .shared_mul_pending(shared_mul_pending),.shared_rop_product(shared_rop_product),.shared_rop_ce(shared_rop_ce),.shared_rop_a(shared_rop_a),.shared_rop_b(shared_rop_b),
        .clk(clk),
        .reset(reset || fault),
"###,
    ),
    (
        r###"        .in_ready(leaf_in_ready),
        .out_valid(leaf_out_valid),
        .out_key(leaf_out_key),
        .new_color(leaf_new_color),
        .new_depth(leaf_new_depth),
"###,
        r###"        .in_ready(leaf_in_ready),
        .out_valid(leaf_out_valid),
        .out_key(),
        .new_color(leaf_new_color),
        .new_depth(leaf_new_depth),
"###,
    ),
    (
        r###"        && (!rop_active || (head_tile==active_tile && !owner0_overlap && !owner1_overlap)));
    // Raw proposal is independent from its validation, so no CE feedback loop.
    wire leaf_proposal = ce && !fault && ((fill_row==0)
        || (row_valid && row_credit && source_ready));
    wire return_proposed = leaf_proposal && leaf_out_valid;
    assign return_bad = return_proposed && (depth_pending || !slot_ready[head]
        || leaf_out_key!=return_key);
    assign source_bad = ingress_error || extra_read || return_bad;
    assign leaf_ce = leaf_proposal && !source_bad && commit_allow;
"###,
        r###"        && (!rop_active || (head_tile==active_tile && !owner0_overlap && !owner1_overlap)));
    // Raw proposal is independent from its validation, so no CE feedback loop.
    wire leaf_proposal = ce && !fault;
    assign shared_rop_offer=leaf_proposal&&shared_mul_pending;
    wire return_proposed = leaf_proposal && leaf_out_valid;
    assign return_bad = return_proposed && (depth_pending || !(slot_ready[head]||(head==tail&&fill_row!=0))
);
    assign source_bad = ingress_error || extra_read || return_bad;
    assign leaf_ce = leaf_proposal && !source_bad && commit_allow;
"###,
    ),
    (
        r###"// registered outputs; no DSP macro mapping, placement or fmax is claimed.
module rop_leaf (
    input             clk,
    input             reset,
"###,
        r###"// registered outputs; no DSP macro mapping, placement or fmax is claimed.
module rop_leaf (
    input wire[53:0]shared_rop_product,output wire shared_mul_pending,shared_rop_ce,output wire[26:0]shared_rop_a,output wire[7:0]shared_rop_b,
    input wire shade_phase,
    input             clk,
    input             reset,
"###,
    ),
    (
        r###"    output reg [1:0]  out_key,
    output reg [15:0] new_color,
    output reg [15:0] new_depth,
    output reg        color_written,
    output reg        depth_written
"###,
        r###"    output reg [1:0]  out_key,
    output reg [15:0] new_color,
    output wire [15:0] new_depth,
    output reg        color_written,
    output reg        depth_written
"###,
    ),
    (
        r###" localparam IDLE=2'd0,SPEC=2'd1,BASE=2'd2,PUBLISH=2'd3;
 reg stopped,result_valid;
 reg [1:0] phase;
 reg [23:0] result;
 // g/h/spec are the parent's immutable live loan.
"###,
        r###" localparam IDLE=2'd0,SPEC=2'd1,BASE=2'd2,PUBLISH=2'd3;
 reg stopped,result_valid;
 reg arithmetic_live;
 wire [1:0] phase=!arithmetic_live?IDLE:(shade_phase?SPEC:BASE);
 reg [23:0] result;
 // g/h/spec are the parent's immutable live loan.
"###,
    ),
    (
        r###" wire occupied=(phase!=IDLE)||result_valid;
 assign fault=stopped||abort;
 assign proposal_ready=!reset&&!fault&&(!occupied||((phase==IDLE||phase==BASE)&&out_ready));
 assign in_ready=running&&proposal_ready;
 assign preflight_bad=proposal_ce&&!reset&&!fault&&proposal_ready&&in_valid&&(in_h>9'd256);
"###,
        r###" wire occupied=(phase!=IDLE)||result_valid;
 assign fault=stopped||abort;
 assign proposal_ready=!shade_phase&&!reset&&!fault&&(!occupied||((phase==IDLE||phase==BASE)&&out_ready));
 assign in_ready=running&&proposal_ready;
 assign preflight_bad=proposal_ce&&!reset&&!fault&&proposal_ready&&in_valid&&(in_h>9'd256);
"###,
    ),
    (
        r###" assign credits={3'b0,occupied};assign queued={3'b0,(result_valid||(phase==BASE))};
 wire publish=advance&&(phase==BASE);
 wire mul_ce=advance&&(in_accept||(phase==SPEC));

"###,
        r###" assign credits={3'b0,occupied};assign queued={3'b0,(result_valid||(phase==BASE))};
 wire publish=advance&&(phase==BASE);
 assign shared_spec_ce=advance&&in_accept;assign shared_spec_a=in_h;assign shared_spec_b=in_spec;
 wire mul_ce=advance&&(in_accept||(phase==SPEC));

"###,
    ),
    (
        concat!(
            r###"  wire [16:0] product;
  "###,
            "\n",
            r###"  reg [8:0] sh;
  wire [15:0] plus128=product[15:0]+16'd128;
  wire [15:0] folded=plus128+{8'd0,plus128[15:8]};
  wire [10:0] sum={1'b0,product[16:7]}+{2'b0,sh};
  wire round_up=sum[0]&&sum[1];
  wire saturated=(|sum[10:9])||(&sum[8:1]);
  wire [7:0] rounded=sum[8:1]+{7'd0,round_up};
  assign color[c*8+:8]=saturated?8'd255:rounded;
  always @*begin
"###
        ),
        concat!(
            r###"  wire [16:0] product;
  "###,
            "\n",
            r###"  reg [7:0] sh;
  wire [15:0] plus128=product[15:0]+16'd128;
  wire [15:0] folded=plus128+{8'd0,plus128[15:8]};
  wire [9:0] sum={1'b0,product[16:8]}+{2'b0,sh};
  wire saturated=(|sum[9:8]);
  wire [7:0] rounded=sum[7:0];
  assign color[c*8+:8]=saturated?8'd255:rounded;
  always @*begin
"###
        ),
    ),
    (
        concat!(
            r###"  gpu_v2_final_two_cycle_mul9 multiplier(.clk(clk),.ce(mul_ce),.a(a),.b(b),.product(product));
  wire [16:0] spec_product;
  gpu_v2_final_two_cycle_mul9 spec_multiplier(.clk(clk),.ce(advance&&in_accept),.a(in_h),.b(in_spec[c*8+:8]),.product(spec_product));
  always @(posedge clk)if(advance)begin
   "###,
            "\n",
            r###"   if(phase==SPEC)sh<=spec_product[15:7];
  end
 end endgenerate
 always @(posedge clk)begin
  if(reset)begin stopped<=0;result_valid<=0;phase<=IDLE;end
  else begin
   if(abort||preflight_bad)stopped<=1;
   if(advance)begin
    case(phase)
     IDLE:if(in_accept)phase<=SPEC;
     SPEC:phase<=BASE;
     BASE:if(in_accept)phase<=SPEC;else phase<=IDLE;
    endcase
    "###,
            "\n",
            r###"    if(publish)begin result<=color;result_valid<=!out_fire;end
"###
        ),
        concat!(
            r###"  gpu_v2_final_two_cycle_mul9 multiplier(.clk(clk),.ce(mul_ce),.a(a),.b(b),.product(product));
  wire [16:0] spec_product;
  assign spec_product=shared_spec_product[c*17+:17];
  always @(posedge clk)if(advance)begin
   "###,
            "\n",
            r###"   if(phase==SPEC)sh<=spec_product[15:8];
  end
 end endgenerate
 always @(posedge clk)begin
  if(reset)begin stopped<=0;result_valid<=0;arithmetic_live<=0;end
  else begin
   if(abort||preflight_bad)stopped<=1;
   if(advance)begin
    if(!shade_phase)arithmetic_live<=in_accept;
    "###,
            "\n",
            r###"    if(publish)begin result<=color;result_valid<=!out_fire;end
"###
        ),
    ),
];
const NATIVE: &[(&str, &str)] = &[
    (
        concat!(
            r###"    endfunction

    // ---- Stage 1: input registers ----
    reg        v1;
    reg [1:0]  k1;
    reg        blend1;
    reg [2:0]  func1;
    reg        dw1;
    reg        covered1;
    reg [15:0] oc1, od1, sd1;
    reg [7:0]  sr1, sg1, sb1, sa1;
    always @(posedge clk) begin
        if (reset) begin
            v1 <= 1'b0;
            k1 <= 2'd0;
        end else if (ce) begin
            v1 <= in_valid;
            k1 <= in_key;
            blend1 <= blend;
            func1 <= depth_func;
            dw1 <= depth_write;
            covered1 <= covered;
            oc1 <= old_color;
            od1 <= old_depth;
            sd1 <= src_depth;
            sr1 <= src_r;
            sg1 <= src_g;
            sb1 <= src_b;
            sa1 <= src_a;
        end
    end

    wire [15:0] compared_old_depth = (k1==0) ? first_old_depth : od1;
    wire [8:0] alpha6_sum = {1'b0,sa1}+9'd2;
    // ---- Stage 2: compare/expand ----
    reg [15:0] sd2;
    reg        v2;
    reg [1:0]  k2;
    reg [7:0]  dr2, dg2, db2, ia2, a2, sr2, sg2, sb2;
    reg        gate2, dw2;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v2 <= 1'b0;
            k2 <= 2'd0;
        end else if (ce) begin
            v2 <= v1;
            k2 <= k1;
            sd2 <= sd1;
            dr2 <= expand5(oc1[15:11]);
            dg2 <= expand6(oc1[10:5]);
            db2 <= expand5(oc1[4:0]);
            a2 <= blend1 ? {1'b0,alpha6_sum[8:2]} : 8'd64;
            ia2 <= blend1 ? 8'd64-{1'b0,alpha6_sum[8:2]} : 8'd0;
            sr2 <= sr1;
            sg2 <= sg1;
            sb2 <= sb1;
            gate2 <= covered1 && depth_pass(func1, sd1, compared_old_depth);
            dw2 <= dw1;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 3: multiply ----
    reg [15:0] sd3;
    reg        v3;
    reg [1:0]  k3;
    wire [15:0] psr3, pdr3, psg3, pdg3, psb3, pdb3;
    reg        gate3, dw3;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v3 <= 1'b0;
            k3 <= 2'd0;
        end else if (ce) begin
            v3 <= v2;
            k3 <= k2;
            sd3 <= sd2;
            gate3 <= gate2;
            dw3 <= dw2;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 4: sum/round ----
    reg [15:0] sd4;
    reg        v4;
    reg [1:0]  k4;
    reg [13:0] nr4, ng4, nb4;
    reg        gate4, dw4;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v4 <= 1'b0;
            k4 <= 2'd0;
        end else if (ce) begin
            v4 <= v3;
            k4 <= k3;
            sd4 <= sd3;
            nr4 <= psr3[13:0] + pdr3[13:0] + 14'd32;
            ng4 <= psg3[13:0] + pdg3[13:0] + 14'd32;
            nb4 <= psb3[13:0] + pdb3[13:0] + 14'd32;
            gate4 <= gate3;
            dw4 <= dw3;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 5: blend divide255 ----
    wire [8:0] div_r4 = {1'b0,nr4[13:6]};
    wire [8:0] div_g4 = {1'b0,ng4[13:6]};
    wire [8:0] div_b4 = {1'b0,nb4[13:6]};
    reg [15:0] sd5;
    reg        v5;
    reg [1:0]  k5;
    reg [7:0]  br5, bg5, bb5;
    reg        gate5, dw5;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v5 <= 1'b0;
            k5 <= 2'd0;
        end else if (ce) begin
            v5 <= v4;
            k5 <= k4;
            sd5 <= sd4;
            br5 <= div_r4[7:0];
            bg5 <= div_g4[7:0];
            bb5 <= div_b4[7:0];
            gate5 <= gate4;
            dw5 <= dw4;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 6: quantize multiply (shifts/subtracts) ----
    reg [15:0] sd6;
    reg        v6;
    reg [1:0]  k6;
    reg [16:0] qmr6, qmg6, qmb6;
    reg        gate6, dw6;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v6 <= 1'b0;
            k6 <= 2'd0;
        end else if (ce) begin
            v6 <= v5;
            k6 <= k5;
            sd6 <= sd5;
            qmr6 <= {12'd0,exactq5(br5)};
            qmg6 <= {11'd0,exactq6(bg5)};
            qmb6 <= {12'd0,exactq5(bb5)};
            gate6 <= gate5;
            dw6 <= dw5;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 7: quantize divide255 ----
    wire [8:0] div_qr = {4'd0,qmr6[4:0]};
    wire [8:0] div_qg = {3'd0,qmg6[5:0]};
    wire [8:0] div_qb = {4'd0,qmb6[4:0]};
    reg [15:0] sd7;
    reg        v7;
    reg [1:0]  k7;
    reg [4:0]  qr7;
    reg [5:0]  qg7;
    reg [4:0]  qb7;
    reg        gate7, dw7;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v7 <= 1'b0;
            k7 <= 2'd0;
        end else if (ce) begin
            v7 <= v6;
            k7 <= k6;
            sd7 <= sd6;
            qr7 <= div_qr[4:0];
            qg7 <= div_qg[5:0];
            qb7 <= div_qb[4:0];
            gate7 <= gate6;
            dw7 <= dw6;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 8: pack/output ----
    always @(posedge clk) begin
        if (reset) begin
            out_valid <= 1'b0;
            out_key <= 2'd0;
            new_color <= 16'd0;
            new_depth <= 16'd0;
            color_written <= 1'b0;
            depth_written <= 1'b0;
        end else if (ce) begin
            out_valid <= v7;
            out_key <= k7;
            color_written <= gate7;
            depth_written <= gate7 && dw7;
            new_color <= {qr7, qg7, qb7}; // Cache consumes only when color_written.
            new_depth <= sd7; // Cache consumes source depth only for a true DW flag.
        end
    end
gpu_v2_rop_mul9 psr3_binding(.clk(clk),.ce(ce),.a(a2),.b(sr2),.product(psr3));
gpu_v2_rop_mul9 pdr3_binding(.clk(clk),.ce(ce),.a(ia2),.b(dr2),.product(pdr3));
gpu_v2_rop_mul9 psg3_binding(.clk(clk),.ce(ce),.a(a2),.b(sg2),.product(psg3));
gpu_v2_rop_mul9 pdg3_binding(.clk(clk),.ce(ce),.a(ia2),.b(dg2),.product(pdg3));
gpu_v2_rop_mul9 psb3_binding(.clk(clk),.ce(ce),.a(a2),.b(sb2),.product(psb3));
gpu_v2_rop_mul9 pdb3_binding(.clk(clk),.ce(ce),.a(ia2),.b(db2),.product(pdb3));
endmodule

module gpu_v2_rop_mul9(input wire clk,ce,input wire[7:0]a,b,output wire[15:0]product);
 wire[17:0]raw;
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b1),
 .ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(raw),.SOA(),.SOB(),.A({1'b0,a}),.B({1'b0,b}),.SIA(9'd0),.SIB(9'd0),
 .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(1'b0));
 assign product=raw[15:0];
endmodule
module gpu_v2_postquad_light_bank(
"###
        ),
        concat!(
            r###"    endfunction

    // Four enabled edges, one pixel every two shared phase edges.
    reg v1,v2,v3;reg[1:0]k1,k2,k3;
    reg first1;reg[15:0]sd1,sd3,od1,oc1;
    assign new_depth=sd3;
    reg[7:0]sr1,sg1,sb1,sa1;
    reg covered1,dw1,blend1;reg[2:0]func1;
    reg gate2,dw2;
    reg[7:0]dr2,dg2,db2;
    reg[7:0]a2;
    reg signed[8:0]delta_r2,delta_g2,delta_b2;
    wire[17:0]delta_r3=shared_rop_product[17:0],delta_g3=shared_rop_product[35:18],delta_b3=shared_rop_product[53:36];
    assign shared_mul_pending=v2;assign shared_rop_ce=ce&&v2;assign shared_rop_a={delta_b2,delta_g2,delta_r2};assign shared_rop_b=a2;
    wire[15:0]compared_old_depth=first1?first_old_depth:od1;
    wire[8:0]alpha_sum={1'b0,sa1}+9'd2;
    wire[13:0]nr3={dr2,6'd0}+delta_r3[13:0]+14'd32;
    wire[13:0]ng3={dg2,6'd0}+delta_g3[13:0]+14'd32;
    wire[13:0]nb3={db2,6'd0}+delta_b3[13:0]+14'd32;
    always @(posedge clk)begin
      if(reset)begin
        v1<=0;v2<=0;v3<=0;k1<=0;k2<=0;k3<=0;
        out_valid<=0;out_key<=0;new_color<=0;color_written<=0;depth_written<=0;
      end else if(ce)begin
        v1<=in_valid;k1<=in_key;first1<=(in_key==0);if(in_valid)sd1<=src_depth;od1<=old_depth;oc1<=old_color;
        sr1<=src_r;sg1<=src_g;sb1<=src_b;sa1<=src_a;
        covered1<=covered;dw1<=depth_write;blend1<=blend;func1<=depth_func;
        v2<=v1;k2<=k1;
        // Phase0 captures once; phase1 borrows these values. The next
        // phase0 consumes the old values before accepting the next pixel.
        if(!shade_phase)begin
          gate2<=covered1&&depth_pass(func1,sd1,compared_old_depth);dw2<=dw1;
          dr2<=expand5(oc1[15:11]);dg2<=expand6(oc1[10:5]);db2<=expand5(oc1[4:0]);
        end
        a2<=blend1?alpha_sum[8:2]:7'd64;
        delta_r2<=$signed({1'b0,sr1})-$signed({1'b0,expand5(oc1[15:11])});
        delta_g2<=$signed({1'b0,sg1})-$signed({1'b0,expand6(oc1[10:5])});
        delta_b2<=$signed({1'b0,sb1})-$signed({1'b0,expand5(oc1[4:0])});
        v3<=v2;k3<=k2;if(shade_phase)sd3<=sd1;
        "###,
            "\n",
            r###"        out_valid<=v3;out_key<=k3;
        color_written<=gate2;depth_written<=gate2&&dw2;
        new_color<={exactq5(nr3[13:6]),exactq6(ng3[13:6]),exactq5(nb3[13:6])};
      end
    end
    "###,
            "\n",
            r###"

endmodule

module gpu_v2_rop_delta_mul9(input wire clk,ce,input wire signed[8:0]a,input wire[6:0]b,output wire[17:0]product);
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b1),.ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(product),.SOA(),.SOB(),.A(a),.B({2'd0,b}),.SIA(9'd0),.SIB(9'd0),.ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b1),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(1'b0));
endmodule
module gpu_v2_postquad_light_bank(
"###
        ),
    ),
    (
        r###" .CEB(read_en && !collision && !reset),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
endmodule
module gpu_v2_postquad_final(input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
 input wire[23:0]in_tint,in_tex,in_spec,input wire[8:0]in_g,in_h,
 output wire in_ready,input wire out_ready,output wire out_valid,
 output wire[23:0]out_rgb,output wire fault,preflight_bad);
 gpu_v2_final_ordered_two_cycle leaf(.clk(clk),.reset(reset),.ce(ce),.proposal_ce(proposal_ce),.commit_allow(commit_allow),.preflight_bad(preflight_bad),.abort(abort),
 .in_valid(in_valid),.in_ready(),.proposal_ready(in_ready),.in_tint(in_tint),.in_tex(in_tex),.in_spec(in_spec),.in_g(in_g),.in_h(in_h),
 .out_ready(out_ready),.out_valid(out_valid),.out_rgb(out_rgb),.fault(fault));endmodule
 // Closed single-result Final; byte zero of each RGB bus is red.
module gpu_v2_final_ordered_two_cycle(
 input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
 input wire [23:0] in_tint,in_tex,in_spec,
 input wire [8:0] in_g,in_h,
"###,
        r###" .CEB(read_en && !collision && !reset),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
endmodule
module gpu_v2_postquad_final(input wire shade_phase,input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
 input wire shared_rop_offer,input wire[50:0]shared_spec_product,output wire shared_spec_ce,output wire[8:0]shared_spec_a,output wire[23:0]shared_spec_b,
 input wire[23:0]in_tint,in_tex,in_spec,input wire[8:0]in_g,in_h,
 output wire in_ready,input wire out_ready,output wire out_valid,
 output wire[23:0]out_rgb,output wire fault,preflight_bad);
 gpu_v2_final_ordered_two_cycle leaf(.shared_rop_offer(shared_rop_offer),.shared_spec_product(shared_spec_product),.shared_spec_ce(shared_spec_ce),.shared_spec_a(shared_spec_a),.shared_spec_b(shared_spec_b),.shade_phase(shade_phase),.clk(clk),.reset(reset),.ce(ce),.proposal_ce(proposal_ce),.commit_allow(commit_allow),.preflight_bad(preflight_bad),.abort(abort),
 .in_valid(in_valid),.in_ready(),.proposal_ready(in_ready),.in_tint(in_tint),.in_tex(in_tex),.in_spec(in_spec),.in_g(in_g),.in_h(in_h),
 .out_ready(out_ready),.out_valid(out_valid),.out_rgb(out_rgb),.fault(fault));endmodule
 // Closed single-result Final; byte zero of each RGB bus is red.
module gpu_v2_final_ordered_two_cycle(
 input wire shade_phase,input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
 input wire shared_rop_offer,input wire[50:0]shared_spec_product,output wire shared_spec_ce,output wire[8:0]shared_spec_a,output wire[23:0]shared_spec_b,
 input wire [23:0] in_tint,in_tex,in_spec,
 input wire [8:0] in_g,in_h,
"###,
    ),
    (
        r###" assign product=raw[16:0];
endmodule
"###,
        r###" assign product=raw[16:0];
endmodule
module gpu_v2_final_rop_bank(input wire clk,reset,rop_ce,spec_ce,input wire[26:0]rop_a,input wire[7:0]rop_b,
 input wire[8:0]spec_a,input wire[23:0]spec_b,output wire[53:0]rop_product,output wire[50:0]spec_product);
 // Both clients advance together. Spec is consumed before the phase1 ROP
 // overwrite; ROP is consumed before the next phase0 spec overwrite.
 genvar c;generate for(c=0;c<3;c=c+1)begin:lanes
 wire[8:0]a=rop_ce?rop_a[c*9+:9]:spec_a;
 wire[7:0]b=rop_ce?rop_b:spec_b[c*8+:8];wire[17:0]raw;
 assign rop_product[c*18+:18]=raw;assign spec_product[c*17+:17]=raw[16:0];
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b1),.ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(raw),.SOA(),.SOB(),.A(a),.B({1'b0,b}),.SIA(9'd0),.SIB(9'd0),.ASEL(1'b0),.BSEL(1'b0),.ASIGN(rop_ce),.BSIGN(1'b0),.CLK(clk),.CE(rop_ce||spec_ce),.RESET(1'b0));
 end endgenerate
endmodule
"###,
    ),
];
const PORTABLE: &[(&str, &str)] = &[
    (
        concat!(
            r###"    endfunction

    // ---- Stage 1: input registers ----
    reg        v1;
    reg [1:0]  k1;
    reg        blend1;
    reg [2:0]  func1;
    reg        dw1;
    reg        covered1;
    reg [15:0] oc1, od1, sd1;
    reg [7:0]  sr1, sg1, sb1, sa1;
    always @(posedge clk) begin
        if (reset) begin
            v1 <= 1'b0;
            k1 <= 2'd0;
        end else if (ce) begin
            v1 <= in_valid;
            k1 <= in_key;
            blend1 <= blend;
            func1 <= depth_func;
            dw1 <= depth_write;
            covered1 <= covered;
            oc1 <= old_color;
            od1 <= old_depth;
            sd1 <= src_depth;
            sr1 <= src_r;
            sg1 <= src_g;
            sb1 <= src_b;
            sa1 <= src_a;
        end
    end

    wire [15:0] compared_old_depth = (k1==0) ? first_old_depth : od1;
    wire [8:0] alpha6_sum = {1'b0,sa1}+9'd2;
    // ---- Stage 2: compare/expand ----
    reg [15:0] sd2;
    reg        v2;
    reg [1:0]  k2;
    reg [7:0]  dr2, dg2, db2, ia2, a2, sr2, sg2, sb2;
    reg        gate2, dw2;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v2 <= 1'b0;
            k2 <= 2'd0;
        end else if (ce) begin
            v2 <= v1;
            k2 <= k1;
            sd2 <= sd1;
            dr2 <= expand5(oc1[15:11]);
            dg2 <= expand6(oc1[10:5]);
            db2 <= expand5(oc1[4:0]);
            a2 <= blend1 ? {1'b0,alpha6_sum[8:2]} : 8'd64;
            ia2 <= blend1 ? 8'd64-{1'b0,alpha6_sum[8:2]} : 8'd0;
            sr2 <= sr1;
            sg2 <= sg1;
            sb2 <= sb1;
            gate2 <= covered1 && depth_pass(func1, sd1, compared_old_depth);
            dw2 <= dw1;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 3: multiply ----
    reg [15:0] sd3;
    reg        v3;
    reg [1:0]  k3;
    reg [15:0] psr3, pdr3, psg3, pdg3, psb3, pdb3;
    reg        gate3, dw3;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v3 <= 1'b0;
            k3 <= 2'd0;
        end else if (ce) begin
            v3 <= v2;
            k3 <= k2;
            sd3 <= sd2;
            psr3 <= {8'd0, a2} * {8'd0, sr2};
            pdr3 <= {8'd0, ia2} * {8'd0, dr2};
            psg3 <= {8'd0, a2} * {8'd0, sg2};
            pdg3 <= {8'd0, ia2} * {8'd0, dg2};
            psb3 <= {8'd0, a2} * {8'd0, sb2};
            pdb3 <= {8'd0, ia2} * {8'd0, db2};
            gate3 <= gate2;
            dw3 <= dw2;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 4: sum/round ----
    reg [15:0] sd4;
    reg        v4;
    reg [1:0]  k4;
    reg [13:0] nr4, ng4, nb4;
    reg        gate4, dw4;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v4 <= 1'b0;
            k4 <= 2'd0;
        end else if (ce) begin
            v4 <= v3;
            k4 <= k3;
            sd4 <= sd3;
            nr4 <= psr3[13:0] + pdr3[13:0] + 14'd32;
            ng4 <= psg3[13:0] + pdg3[13:0] + 14'd32;
            nb4 <= psb3[13:0] + pdb3[13:0] + 14'd32;
            gate4 <= gate3;
            dw4 <= dw3;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 5: blend divide255 ----
    wire [8:0] div_r4 = {1'b0,nr4[13:6]};
    wire [8:0] div_g4 = {1'b0,ng4[13:6]};
    wire [8:0] div_b4 = {1'b0,nb4[13:6]};
    reg [15:0] sd5;
    reg        v5;
    reg [1:0]  k5;
    reg [7:0]  br5, bg5, bb5;
    reg        gate5, dw5;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v5 <= 1'b0;
            k5 <= 2'd0;
        end else if (ce) begin
            v5 <= v4;
            k5 <= k4;
            sd5 <= sd4;
            br5 <= div_r4[7:0];
            bg5 <= div_g4[7:0];
            bb5 <= div_b4[7:0];
            gate5 <= gate4;
            dw5 <= dw4;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 6: quantize multiply (shifts/subtracts) ----
    reg [15:0] sd6;
    reg        v6;
    reg [1:0]  k6;
    reg [16:0] qmr6, qmg6, qmb6;
    reg        gate6, dw6;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v6 <= 1'b0;
            k6 <= 2'd0;
        end else if (ce) begin
            v6 <= v5;
            k6 <= k5;
            sd6 <= sd5;
            qmr6 <= {12'd0,exactq5(br5)};
            qmg6 <= {11'd0,exactq6(bg5)};
            qmb6 <= {12'd0,exactq5(bb5)};
            gate6 <= gate5;
            dw6 <= dw5;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 7: quantize divide255 ----
    wire [8:0] div_qr = {4'd0,qmr6[4:0]};
    wire [8:0] div_qg = {3'd0,qmg6[5:0]};
    wire [8:0] div_qb = {4'd0,qmb6[4:0]};
    reg [15:0] sd7;
    reg        v7;
    reg [1:0]  k7;
    reg [4:0]  qr7;
    reg [5:0]  qg7;
    reg [4:0]  qb7;
    reg        gate7, dw7;
    "###,
            "\n",
            r###"    always @(posedge clk) begin
        if (reset) begin
            v7 <= 1'b0;
            k7 <= 2'd0;
        end else if (ce) begin
            v7 <= v6;
            k7 <= k6;
            sd7 <= sd6;
            qr7 <= div_qr[4:0];
            qg7 <= div_qg[5:0];
            qb7 <= div_qb[4:0];
            gate7 <= gate6;
            dw7 <= dw6;
            // Old color remains in cache; a disabled write needs no fallback.
            // Depth payload remains in the active cache row owner.
            // Source depth is borrowed only at the actual keyed return.
        end
    end

    // ---- Stage 8: pack/output ----
    always @(posedge clk) begin
        if (reset) begin
            out_valid <= 1'b0;
            out_key <= 2'd0;
            new_color <= 16'd0;
            new_depth <= 16'd0;
            color_written <= 1'b0;
            depth_written <= 1'b0;
        end else if (ce) begin
            out_valid <= v7;
            out_key <= k7;
            color_written <= gate7;
            depth_written <= gate7 && dw7;
            new_color <= {qr7, qg7, qb7}; // Cache consumes only when color_written.
            new_depth <= sd7; // Cache consumes source depth only for a true DW flag.
        end
    end
endmodule
module gpu_v2_postquad_light_bank(
"###
        ),
        concat!(
            r###"    endfunction

    // Four enabled edges, one pixel every two shared phase edges.
    reg v1,v2,v3;reg[1:0]k1,k2,k3;
    reg first1;reg[15:0]sd1,sd3,od1,oc1;
    assign new_depth=sd3;
    reg[7:0]sr1,sg1,sb1,sa1;
    reg covered1,dw1,blend1;reg[2:0]func1;
    reg gate2,dw2;
    reg[7:0]dr2,dg2,db2;
    reg[7:0]a2;
    reg signed[8:0]delta_r2,delta_g2,delta_b2;
    wire[17:0]delta_r3=shared_rop_product[17:0],delta_g3=shared_rop_product[35:18],delta_b3=shared_rop_product[53:36];
    assign shared_mul_pending=v2;assign shared_rop_ce=ce&&v2;assign shared_rop_a={delta_b2,delta_g2,delta_r2};assign shared_rop_b=a2;
    wire[15:0]compared_old_depth=first1?first_old_depth:od1;
    wire[8:0]alpha_sum={1'b0,sa1}+9'd2;
    wire[13:0]nr3={dr2,6'd0}+delta_r3[13:0]+14'd32;
    wire[13:0]ng3={dg2,6'd0}+delta_g3[13:0]+14'd32;
    wire[13:0]nb3={db2,6'd0}+delta_b3[13:0]+14'd32;
    always @(posedge clk)begin
      if(reset)begin
        v1<=0;v2<=0;v3<=0;k1<=0;k2<=0;k3<=0;
        out_valid<=0;out_key<=0;new_color<=0;color_written<=0;depth_written<=0;
      end else if(ce)begin
        v1<=in_valid;k1<=in_key;first1<=(in_key==0);if(in_valid)sd1<=src_depth;od1<=old_depth;oc1<=old_color;
        sr1<=src_r;sg1<=src_g;sb1<=src_b;sa1<=src_a;
        covered1<=covered;dw1<=depth_write;blend1<=blend;func1<=depth_func;
        v2<=v1;k2<=k1;
        // Phase0 captures once; phase1 borrows these values. The next
        // phase0 consumes the old values before accepting the next pixel.
        if(!shade_phase)begin
          gate2<=covered1&&depth_pass(func1,sd1,compared_old_depth);dw2<=dw1;
          dr2<=expand5(oc1[15:11]);dg2<=expand6(oc1[10:5]);db2<=expand5(oc1[4:0]);
        end
        a2<=blend1?alpha_sum[8:2]:7'd64;
        delta_r2<=$signed({1'b0,sr1})-$signed({1'b0,expand5(oc1[15:11])});
        delta_g2<=$signed({1'b0,sg1})-$signed({1'b0,expand6(oc1[10:5])});
        delta_b2<=$signed({1'b0,sb1})-$signed({1'b0,expand5(oc1[4:0])});
        v3<=v2;k3<=k2;if(shade_phase)sd3<=sd1;
                out_valid<=v3;out_key<=k3;
        color_written<=gate2;depth_written<=gate2&&dw2;
        new_color<={exactq5(nr3[13:6]),exactq6(ng3[13:6]),exactq5(nb3[13:6])};
      end
    end
    "###,
            "\n",
            r###"endmodule
module gpu_v2_postquad_light_bank(
"###
        ),
    ),
    (
        r###" end
endmodule
module gpu_v2_postquad_final(input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
 input wire[23:0]in_tint,in_tex,in_spec,input wire[8:0]in_g,in_h,
 output wire in_ready,input wire out_ready,output wire out_valid,
 output wire[23:0]out_rgb,output wire fault,preflight_bad);
 gpu_v2_final_ordered_two_cycle leaf(.clk(clk),.reset(reset),.ce(ce),.proposal_ce(proposal_ce),.commit_allow(commit_allow),.preflight_bad(preflight_bad),.abort(abort),
 .in_valid(in_valid),.in_ready(),.proposal_ready(in_ready),.in_tint(in_tint),.in_tex(in_tex),.in_spec(in_spec),.in_g(in_g),.in_h(in_h),
 .out_ready(out_ready),.out_valid(out_valid),.out_rgb(out_rgb),.fault(fault));endmodule
 // Closed single-result Final; byte zero of each RGB bus is red.
module gpu_v2_final_ordered_two_cycle(
 input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
 input wire [23:0] in_tint,in_tex,in_spec,
 input wire [8:0] in_g,in_h,
"###,
        r###" end
endmodule
module gpu_v2_postquad_final(input wire shade_phase,input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
 input wire shared_rop_offer,input wire[50:0]shared_spec_product,output wire shared_spec_ce,output wire[8:0]shared_spec_a,output wire[23:0]shared_spec_b,
 input wire[23:0]in_tint,in_tex,in_spec,input wire[8:0]in_g,in_h,
 output wire in_ready,input wire out_ready,output wire out_valid,
 output wire[23:0]out_rgb,output wire fault,preflight_bad);
 gpu_v2_final_ordered_two_cycle leaf(.shared_rop_offer(shared_rop_offer),.shared_spec_product(shared_spec_product),.shared_spec_ce(shared_spec_ce),.shared_spec_a(shared_spec_a),.shared_spec_b(shared_spec_b),.shade_phase(shade_phase),.clk(clk),.reset(reset),.ce(ce),.proposal_ce(proposal_ce),.commit_allow(commit_allow),.preflight_bad(preflight_bad),.abort(abort),
 .in_valid(in_valid),.in_ready(),.proposal_ready(in_ready),.in_tint(in_tint),.in_tex(in_tex),.in_spec(in_spec),.in_g(in_g),.in_h(in_h),
 .out_ready(out_ready),.out_valid(out_valid),.out_rgb(out_rgb),.fault(fault));endmodule
 // Closed single-result Final; byte zero of each RGB bus is red.
module gpu_v2_final_ordered_two_cycle(
 input wire shade_phase,input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,
 input wire shared_rop_offer,input wire[50:0]shared_spec_product,output wire shared_spec_ce,output wire[8:0]shared_spec_a,output wire[23:0]shared_spec_b,
 input wire [23:0] in_tint,in_tex,in_spec,
 input wire [8:0] in_g,in_h,
"###,
    ),
    (
        r###" assign product=q;
endmodule
"###,
        r###" assign product=q;
endmodule
module gpu_v2_final_rop_bank(input wire clk,reset,rop_ce,spec_ce,input wire[26:0]rop_a,input wire[7:0]rop_b,
 input wire[8:0]spec_a,input wire[23:0]spec_b,output wire[53:0]rop_product,output wire[50:0]spec_product);
 // Both clients advance together. Spec is consumed before the phase1 ROP
 // overwrite; ROP is consumed before the next phase0 spec overwrite.
 genvar c;generate for(c=0;c<3;c=c+1)begin:lanes
 wire[8:0]a=rop_ce?rop_a[c*9+:9]:spec_a;
 wire[7:0]b=rop_ce?rop_b:spec_b[c*8+:8];wire[17:0]raw;
 assign rop_product[c*18+:18]=raw;assign spec_product[c*17+:17]=raw[16:0];
 reg[17:0]p;assign raw=p;always @(posedge clk)if(rop_ce||spec_ce)begin if(rop_ce)p<=$signed(a)*$signed({1'b0,b});else p<=a*b;end
 end endgenerate
endmodule
"###,
    ),
];
