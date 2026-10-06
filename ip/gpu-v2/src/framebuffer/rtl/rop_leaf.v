// Independent exact ROP arithmetic leaf, registered pipeline.
//
// Eight clocked arithmetic stages, one lane per enabled edge, no initial blocks
// and no general divider:
//   1 Input          register the lane boundary (valid, key and operands)
//   2 CompareExpand  D16 compare, coverage gate, RGB565 bit-replication expand
//   3 Multiply       six variable 8x8 products: A*S and (255-A)*D per channel
//   4 SumRound       per-channel sum plus the 127 round correction
//   5 Divide255      bounded shift/add/increment divide (REPLACE bypasses)
//   6 QuantizeMul    constant 31/63 scale as shifts/subtracts plus 127
//   7 QuantizeDiv    bounded divide by 255
//   8 Pack           RGB565 pack, gate and result registers
//
// CE edge contract. Every stage register and the valid/key pipeline advance only
// on an enabled edge (`clk` with `ce=1`); with `ce=0` all eight stages, the
// valid bits and the output payload freeze. `reset` clears every valid bit and
// the published result regardless of `ce`.
//
// Edge convention (matches the Rust register emulator and its certificate):
// a lane offered on an edge with `in_valid && ce` is captured by stage 1 at the
// end of that edge. Stage 8 publishes at the end of the eighth edge after
// acceptance, so the result and `out_valid` are observed before the next edge.
// With the lane accepted on enabled edge 1, stage 1 captures at the end of edge
// 1 and the pack stage publishes `out_valid` at the end of edge 8; the result is
// observed pre-edge 9. `in_ready` is `ce`: this leaf has no FIFO and consumes
// one accepted lane per enabled edge; the cache reserves the four return
// destinations.
//
// Division by 255 is the bounded shift/add/increment identity, never a general
// divider. The six `*` operators are ordinary combinational 8x8 products with
// registered outputs; no DSP macro mapping, placement or fmax is claimed.
module rop_leaf (
    input             clk,
    input             reset,
    input             ce,
    input             in_valid,
    input      [1:0]  in_key,
    input             blend,        // 0 = REPLACE, 1 = SRC_OVER
    input      [2:0]  depth_func,
    input             depth_write,
    input             covered,
    input      [15:0] old_color,
    input      [15:0] old_depth,
    input      [7:0]  src_r,
    input      [7:0]  src_g,
    input      [7:0]  src_b,
    input      [7:0]  src_a,
    input      [15:0] src_depth,
    output wire       in_ready,
    output reg        out_valid,
    output reg [1:0]  out_key,
    output reg [15:0] new_color,
    output reg [15:0] new_depth,
    output reg        color_written,
    output reg        depth_written
);
    // No output FIFO and no backpressure: one lane is accepted per enabled edge.
    assign in_ready = ce;

    function [8:0] div255;
        input [16:0] n;
        reg [17:0] t;
        begin
            t = {1'b0, n} + {9'b0, n[16:8]} + 18'd1;
            div255 = t[16:8];
        end
    endfunction

    function [7:0] expand5;
        input [4:0] c;
        begin
            expand5 = {c, c[4:2]};
        end
    endfunction

    function [7:0] expand6;
        input [5:0] c;
        begin
            expand6 = {c, c[5:4]};
        end
    endfunction

    function depth_pass;
        input [2:0]  f;
        input [15:0] s;
        input [15:0] o;
        begin
            case (f)
                3'd0: depth_pass = 1'b0;
                3'd1: depth_pass = (s < o);
                3'd2: depth_pass = (s == o);
                3'd3: depth_pass = (s <= o);
                3'd4: depth_pass = (s > o);
                3'd5: depth_pass = (s != o);
                3'd6: depth_pass = (s >= o);
                default: depth_pass = 1'b1;
            endcase
        end
    endfunction

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

    // ---- Stage 2: compare/expand ----
    reg        v2;
    reg [1:0]  k2;
    reg [7:0]  dr2, dg2, db2, ia2, a2, sr2, sg2, sb2;
    reg        gate2, dw2, blend2;
    reg [15:0] oc2, od2, sd2;
    always @(posedge clk) begin
        if (reset) begin
            v2 <= 1'b0;
            k2 <= 2'd0;
        end else if (ce) begin
            v2 <= v1;
            k2 <= k1;
            dr2 <= expand5(oc1[15:11]);
            dg2 <= expand6(oc1[10:5]);
            db2 <= expand5(oc1[4:0]);
            a2 <= sa1;
            ia2 <= 8'd255 - sa1;
            sr2 <= sr1;
            sg2 <= sg1;
            sb2 <= sb1;
            gate2 <= covered1 && depth_pass(func1, sd1, od1);
            dw2 <= dw1;
            blend2 <= blend1;
            oc2 <= oc1;
            od2 <= od1;
            sd2 <= sd1;
        end
    end

    // ---- Stage 3: multiply ----
    reg        v3;
    reg [1:0]  k3;
    reg [15:0] psr3, pdr3, psg3, pdg3, psb3, pdb3;
    reg        gate3, dw3, blend3;
    reg [7:0]  sr3, sg3, sb3;
    reg [15:0] oc3, od3, sd3;
    always @(posedge clk) begin
        if (reset) begin
            v3 <= 1'b0;
            k3 <= 2'd0;
        end else if (ce) begin
            v3 <= v2;
            k3 <= k2;
            psr3 <= {8'd0, a2} * {8'd0, sr2};
            pdr3 <= {8'd0, ia2} * {8'd0, dr2};
            psg3 <= {8'd0, a2} * {8'd0, sg2};
            pdg3 <= {8'd0, ia2} * {8'd0, dg2};
            psb3 <= {8'd0, a2} * {8'd0, sb2};
            pdb3 <= {8'd0, ia2} * {8'd0, db2};
            gate3 <= gate2;
            dw3 <= dw2;
            blend3 <= blend2;
            sr3 <= sr2;
            sg3 <= sg2;
            sb3 <= sb2;
            oc3 <= oc2;
            od3 <= od2;
            sd3 <= sd2;
        end
    end

    // ---- Stage 4: sum/round ----
    reg        v4;
    reg [1:0]  k4;
    reg [16:0] nr4, ng4, nb4;
    reg        gate4, dw4, blend4;
    reg [7:0]  sr4, sg4, sb4;
    reg [15:0] oc4, od4, sd4;
    always @(posedge clk) begin
        if (reset) begin
            v4 <= 1'b0;
            k4 <= 2'd0;
        end else if (ce) begin
            v4 <= v3;
            k4 <= k3;
            nr4 <= psr3 + pdr3 + 16'd127;
            ng4 <= psg3 + pdg3 + 16'd127;
            nb4 <= psb3 + pdb3 + 16'd127;
            gate4 <= gate3;
            dw4 <= dw3;
            blend4 <= blend3;
            sr4 <= sr3;
            sg4 <= sg3;
            sb4 <= sb3;
            oc4 <= oc3;
            od4 <= od3;
            sd4 <= sd3;
        end
    end

    // ---- Stage 5: blend divide255 ----
    wire [8:0] div_r4 = div255(nr4);
    wire [8:0] div_g4 = div255(ng4);
    wire [8:0] div_b4 = div255(nb4);
    reg        v5;
    reg [1:0]  k5;
    reg [7:0]  br5, bg5, bb5;
    reg        gate5, dw5;
    reg [15:0] oc5, od5, sd5;
    always @(posedge clk) begin
        if (reset) begin
            v5 <= 1'b0;
            k5 <= 2'd0;
        end else if (ce) begin
            v5 <= v4;
            k5 <= k4;
            br5 <= blend4 ? div_r4[7:0] : sr4;
            bg5 <= blend4 ? div_g4[7:0] : sg4;
            bb5 <= blend4 ? div_b4[7:0] : sb4;
            gate5 <= gate4;
            dw5 <= dw4;
            oc5 <= oc4;
            od5 <= od4;
            sd5 <= sd4;
        end
    end

    // ---- Stage 6: quantize multiply (shifts/subtracts) ----
    reg        v6;
    reg [1:0]  k6;
    reg [16:0] qmr6, qmg6, qmb6;
    reg        gate6, dw6;
    reg [15:0] oc6, od6, sd6;
    always @(posedge clk) begin
        if (reset) begin
            v6 <= 1'b0;
            k6 <= 2'd0;
        end else if (ce) begin
            v6 <= v5;
            k6 <= k5;
            qmr6 <= ({9'd0, br5} << 5) - {9'd0, br5} + 17'd127;
            qmg6 <= ({9'd0, bg5} << 6) - {9'd0, bg5} + 17'd127;
            qmb6 <= ({9'd0, bb5} << 5) - {9'd0, bb5} + 17'd127;
            gate6 <= gate5;
            dw6 <= dw5;
            oc6 <= oc5;
            od6 <= od5;
            sd6 <= sd5;
        end
    end

    // ---- Stage 7: quantize divide255 ----
    wire [8:0] div_qr = div255(qmr6);
    wire [8:0] div_qg = div255(qmg6);
    wire [8:0] div_qb = div255(qmb6);
    reg        v7;
    reg [1:0]  k7;
    reg [4:0]  qr7;
    reg [5:0]  qg7;
    reg [4:0]  qb7;
    reg        gate7, dw7;
    reg [15:0] oc7, od7, sd7;
    always @(posedge clk) begin
        if (reset) begin
            v7 <= 1'b0;
            k7 <= 2'd0;
        end else if (ce) begin
            v7 <= v6;
            k7 <= k6;
            qr7 <= div_qr[4:0];
            qg7 <= div_qg[5:0];
            qb7 <= div_qb[4:0];
            gate7 <= gate6;
            dw7 <= dw6;
            oc7 <= oc6;
            od7 <= od6;
            sd7 <= sd6;
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
            new_color <= gate7 ? {qr7, qg7, qb7} : oc7;
            new_depth <= (gate7 && dw7) ? sd7 : od7;
        end
    end
endmodule
