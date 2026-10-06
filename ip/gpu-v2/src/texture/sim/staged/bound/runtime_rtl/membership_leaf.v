// Synthesizable scalar membership leaf: the registered 7-stage pipeline of
// `runtime_membership.rs`, one lane per enabled edge, no FIFO, no work credit.
//
//   E0 input       weights 4x9, coordinates 4x10, slot4/level4/key6, fine/lastFine
//   E1 slice       tile x/y (c>>3), local x/y (c&7), nonzero weight flags
//   E2 equal       same_x (tx0==tx1), same_y (ty0==ty1)
//   E3 pairs       retained pair equivalences [01,02,03,12,13,23]
//   E4 emit        representative emit0..3 and the exact 92-bit Member word
//   E5 align       registered alignment stage
//   E6 output      registered alignment stage; old E6 publishes on E7
//
// CE contract: every valid bit and data register advances only on an enabled
// edge (`clk` with `ce=1`); `ce=0` freezes the whole pipeline, including the
// seven valid bits and the published word. `reset` clears the valid bits, the
// fault and the published word.
//
// Fault contract: the only runtime violation an exact-width input can express is
// an inactive plane, i.e. all four weights zero. On the offending enabled edge
// the datapath does not advance, `fault` latches, and `in_ready`/`out_valid`
// are driven low until reset. There is no output FIFO and no backpressure:
// while unfaulted `in_ready` is `ce`, so the caller reserves the Work credit
// externally and one active plane is accepted per enabled edge.
//
// The 92-bit word uses the frozen transport Member encoding, so it is the same
// scalar `Member` that the packet leaf consumes. No counted/oracle helper is
// called and no captured stage value is read back: every bit is computed from
// the lane operands registered in this module.

module membership_leaf (
    input             clk,
    input             reset,
    input             ce,
    input             in_valid,
    input      [8:0]  in_w0,
    input      [8:0]  in_w1,
    input      [8:0]  in_w2,
    input      [8:0]  in_w3,
    input      [9:0]  in_c0,
    input      [9:0]  in_c1,
    input      [9:0]  in_c2,
    input      [9:0]  in_c3,
    input      [3:0]  in_slot,
    input      [3:0]  in_level,
    input      [5:0]  in_key,
    input             in_fine,
    input             in_last_fine,
    output wire       in_ready,
    output wire       out_valid,
    output reg [91:0] out_member,
    output reg        fault
);

    // Elaboration-time constant (named override at instantiation): 0 keeps the
    // accepted E0..E6 alignment and 7-edge latency bit-for-bit; 1 publishes the
    // E4 arithmetic register at 5 enabled edges and lets synthesis prune the two
    // unused 92-bit banks.
    parameter SHORT_ALIGNMENT = 0;

    // Inactive plane is the only unrepresentable input an exact-width lane can
    // express; the width checks of the private model are structural here.
    wire bad_plane = (in_w0 | in_w1 | in_w2 | in_w3) == 9'd0;
    wire do_fault  = ce & ~fault & in_valid & bad_plane;
    wire advance   = ce & ~fault & ~do_fault;

    // ---- E0: input capture ----
    reg        v0;
    reg [8:0]  w0_0, w1_0, w2_0, w3_0;
    reg [9:0]  c0_0, c1_0, c2_0, c3_0;
    reg [3:0]  slot0, level0;
    reg [5:0]  key0;
    reg        fine0, lf0;

    // ---- E1: tile/local slice and nonzero flags ----
    reg        v1;
    reg [8:0]  w0_1, w1_1, w2_1, w3_1;
    reg [6:0]  tx0_1, tx1_1, ty0_1, ty1_1;
    reg [2:0]  lx1, ly1;
    reg [3:0]  slot1, level1;
    reg [5:0]  key1;
    reg        fine1, final1;
    reg        nz0_1, nz1_1, nz2_1, nz3_1;

    // ---- E2: same-tile comparisons ----
    reg        v2;
    reg [8:0]  w0_2, w1_2, w2_2, w3_2;
    reg [6:0]  tx0_2, tx1_2, ty0_2, ty1_2;
    reg [2:0]  lx2, ly2;
    reg [3:0]  slot2, level2;
    reg [5:0]  key2;
    reg        fine2, final2;
    reg        nz0_2, nz1_2, nz2_2, nz3_2;
    reg        same_x2, same_y2;

    // ---- E3: retained pair equivalences ----
    reg        v3;
    reg [8:0]  w0_3, w1_3, w2_3, w3_3;
    reg [6:0]  tx0_3, tx1_3, ty0_3, ty1_3;
    reg [2:0]  lx3, ly3;
    reg [3:0]  slot3, level3;
    reg [5:0]  key3;
    reg        fine3, final3;
    reg        nz0_3, nz1_3, nz2_3, nz3_3;
    reg        same_x3, same_y3;
    reg        p0_3, p1_3, p2_3, p3_3, p4_3, p5_3;

    // ---- E4: representative emit selection and Member word ----
    reg        v4;
    reg [91:0] member4;

    // ---- E5/E6: default-only alignment tail ----
    reg        v5;
    reg [91:0] member5;

    // ---- E6: published output ----
    reg        v6;

    assign in_ready  = ce & ~fault;
    // Short alignment publishes the E4 register; the default keeps the old E6.
    assign out_valid = SHORT_ALIGNMENT ? (v4 & ~fault) : (v6 & ~fault);

    // Representative selection: the first nonzero weight of each tile is kept,
    // duplicate-tile coefficients below it are suppressed (never merged).
    wire e0 = nz0_3;
    wire e1 = nz1_3 & ~(nz0_3 & p0_3);
    wire e2 = nz2_3 & ~((nz0_3 & p1_3) | (nz1_3 & p3_3));
    wire e3 = nz3_3 & ~((nz0_3 & p2_3) | (nz1_3 & p4_3) | (nz2_3 & p5_3));

    wire [91:0] member_next = {
        final3,          // 91
        fine3,           // 90
        key3[1:0],       // 89:88 lane
        key3[5:2],       // 87:84 quad
        level3,          // 83:80 n
        slot3,           // 79:76
        same_y3,         // 75
        same_x3,         // 74
        ly3,             // 73:71
        lx3,             // 70:68
        ty1_3,           // 67:61
        ty0_3,           // 60:54
        tx1_3,           // 53:47
        tx0_3,           // 46:40
        e3, e2, e1, e0,  // 39:36
        w3_3, w2_3, w1_3, w0_3
    };

    always @(posedge clk) begin
        if (reset) begin
            fault     <= 1'b0;
            v0 <= 1'b0;
            v1 <= 1'b0;
            v2 <= 1'b0;
            v3 <= 1'b0;
            v4 <= 1'b0;
            v5 <= 1'b0;
            v6 <= 1'b0;
            out_member <= 92'd0;
        end else begin
            if (do_fault) begin
                fault <= 1'b1;
            end
            if (advance) begin
                // E0
                v0     <= in_valid;
                w0_0   <= in_w0;
                w1_0   <= in_w1;
                w2_0   <= in_w2;
                w3_0   <= in_w3;
                c0_0   <= in_c0;
                c1_0   <= in_c1;
                c2_0   <= in_c2;
                c3_0   <= in_c3;
                slot0  <= in_slot;
                level0 <= in_level;
                key0   <= in_key;
                fine0  <= in_fine;
                lf0    <= in_last_fine;
                // E1
                v1     <= v0;
                w0_1   <= w0_0;
                w1_1   <= w1_0;
                w2_1   <= w2_0;
                w3_1   <= w3_0;
                tx0_1  <= c0_0[9:3];
                tx1_1  <= c1_0[9:3];
                ty0_1  <= c2_0[9:3];
                ty1_1  <= c3_0[9:3];
                lx1    <= c0_0[2:0];
                ly1    <= c2_0[2:0];
                slot1  <= slot0;
                level1 <= level0;
                key1   <= key0;
                fine1  <= fine0;
                final1 <= ~fine0 | lf0;
                nz0_1  <= (w0_0 != 9'd0);
                nz1_1  <= (w1_0 != 9'd0);
                nz2_1  <= (w2_0 != 9'd0);
                nz3_1  <= (w3_0 != 9'd0);
                // E2
                v2      <= v1;
                w0_2    <= w0_1;
                w1_2    <= w1_1;
                w2_2    <= w2_1;
                w3_2    <= w3_1;
                tx0_2   <= tx0_1;
                tx1_2   <= tx1_1;
                ty0_2   <= ty0_1;
                ty1_2   <= ty1_1;
                lx2     <= lx1;
                ly2     <= ly1;
                slot2   <= slot1;
                level2  <= level1;
                key2    <= key1;
                fine2   <= fine1;
                final2  <= final1;
                nz0_2   <= nz0_1;
                nz1_2   <= nz1_1;
                nz2_2   <= nz2_1;
                nz3_2   <= nz3_1;
                same_x2 <= (tx0_1 == tx1_1);
                same_y2 <= (ty0_1 == ty1_1);
                // E3
                v3      <= v2;
                w0_3    <= w0_2;
                w1_3    <= w1_2;
                w2_3    <= w2_2;
                w3_3    <= w3_2;
                tx0_3   <= tx0_2;
                tx1_3   <= tx1_2;
                ty0_3   <= ty0_2;
                ty1_3   <= ty1_2;
                lx3     <= lx2;
                ly3     <= ly2;
                slot3   <= slot2;
                level3  <= level2;
                key3    <= key2;
                fine3   <= fine2;
                final3  <= final2;
                nz0_3   <= nz0_2;
                nz1_3   <= nz1_2;
                nz2_3   <= nz2_2;
                nz3_3   <= nz3_2;
                same_x3 <= same_x2;
                same_y3 <= same_y2;
                p0_3    <= same_x2;
                p1_3    <= same_y2;
                p2_3    <= same_x2 & same_y2;
                p3_3    <= same_x2 & same_y2;
                p4_3    <= same_y2;
                p5_3    <= same_x2;
                // E4
                v4      <= v3;
                member4 <= member_next;
                // E5/E6 default-only alignment tail; the short constant publishes
                // the E4 arithmetic word directly, aligned with v4, so synthesis
                // removes the tail without a skew between valid and data.
                if (SHORT_ALIGNMENT) begin
                    out_member <= member_next;
                end else begin
                    // E5
                    v5      <= v4;
                    member5 <= member4;
                    // E6
                    v6         <= v5;
                    out_member <= member5;
                end
            end
        end
    end

endmodule
