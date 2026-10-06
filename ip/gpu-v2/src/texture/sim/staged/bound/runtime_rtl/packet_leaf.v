// Synthesizable scalar packet leaf: the registered 9-stage pipeline of
// `runtime_packet.rs`, one emitted tap per enabled edge, no FIFO, no Pool credit.
//
//   E0 input     validates the emitted tap and captures Member92+tap2 as 93 bits
//                (the emit0 bit is dropped after its last read at the predicate)
//   E1 selected  header28, four 9-bit weights, key6, masks4, first, last (76 bits)
//   E2 pack      four masked 9-bit weights concatenated with header/flags (72 bits)
//   E3..E8       seven 72-bit alignment banks (E2..E8 inclusive)
//
// CE contract: every valid bit and bank advances only on an enabled edge
// (`clk` with `ce=1`); `ce=0` freezes the whole pipeline, including the nine
// valid bits and the published word. `reset` clears the valid bits, the fault
// and the published word. The output is the E8 bank; old E8 publishes on E9.
//
// Fault contract: `in_valid` with a tap whose Member emit bit is zero is
// invalid. On the offending enabled edge the datapath does not advance, `fault`
// latches, and `in_ready`/`out_valid` are driven low until reset. While
// unfaulted `in_ready` is `ce`, so the caller reserves the Pool64 credit
// externally and one emitted tap is accepted per enabled edge.
//
// Every field is extracted from the operands registered inside this module.
// The leaf never reads a captured stage value, an oracle answer or a counted
// helper; the four 9-bit weight fields are disjoint concatenations, not adds.

module packet_leaf (
    input             clk,
    input             reset,
    input             ce,
    input             in_valid,
    input      [91:0] in_member,
    input      [1:0]  in_tap,
    output wire       in_ready,
    output wire       out_valid,
    output reg [71:0] out_packet,
    output reg        fault
);

    // Elaboration-time constant (named override at instantiation): 0 keeps the
    // accepted E0..E8 alignment and 9-edge latency bit-for-bit; 1 publishes the
    // E2 pack register at 3 enabled edges and lets synthesis prune the six
    // unused 72-bit banks.
    parameter SHORT_ALIGNMENT = 0;

    // Emitted predicate: the tap must select a set Member emit bit.
    wire [3:0] emit_in = in_member[39:36];
    wire emitted = (in_tap == 2'd0) ? emit_in[0]
                 : (in_tap == 2'd1) ? emit_in[1]
                 : (in_tap == 2'd2) ? emit_in[2]
                 :                   emit_in[3];
    wire do_fault = ce & ~fault & in_valid & ~emitted;
    wire advance  = ce & ~fault & ~do_fault;

    // E0 capture: drop emit0 (member bit 36) after the predicate, place tap2.
    wire [92:0] cap_next = { in_tap, in_member[91:37], in_member[35:0] };
    reg  [92:0] cap_e0;
    reg         v0;

    // E1 selection from the registered E0 word.
    wire [1:0] sel_tap = cap_e0[92:91];
    wire       sel_x   = cap_e0[91];
    wire       sel_y   = cap_e0[92];
    wire       same_x  = cap_e0[73];
    wire       same_y  = cap_e0[74];
    wire [6:0] tile_x  = sel_x ? cap_e0[52:46] : cap_e0[45:39];
    wire [6:0] tile_y  = sel_y ? cap_e0[66:60] : cap_e0[59:53];
    wire [27:0] header = {
        cap_e0[72:70],   // 27:25 ly
        cap_e0[69:67],   // 24:22 lx
        tile_y,          // 21:15
        tile_x,          // 14:8
        cap_e0[82:79],   // 7:4   n
        cap_e0[78:75]    // 3:0   slot
    };
    wire [8:0] sel_w0 = cap_e0[8:0];
    wire [8:0] sel_w1 = cap_e0[17:9];
    wire [8:0] sel_w2 = cap_e0[26:18];
    wire [8:0] sel_w3 = cap_e0[35:27];
    wire [5:0] sel_key = { cap_e0[86:83], cap_e0[88:87] };
    wire sel_e1b = cap_e0[36];
    wire sel_e2b = cap_e0[37];
    wire sel_e3b = cap_e0[38];
    wire higher = (sel_tap == 2'd0) ? (sel_e1b | sel_e2b | sel_e3b)
                : (sel_tap == 2'd1) ? (sel_e2b | sel_e3b)
                : (sel_tap == 2'd2) ?  sel_e3b
                :                     1'b0;
    wire sel_first = cap_e0[89] & (sel_tap == 2'd0);
    wire sel_last  = cap_e0[90] & ~higher;
    wire [3:0] sel_masks;
    assign sel_masks[0] = (sel_x == 1'b0 || same_x) && (sel_y == 1'b0 || same_y);
    assign sel_masks[1] = (sel_x == 1'b1 || same_x) && (sel_y == 1'b0 || same_y);
    assign sel_masks[2] = (sel_x == 1'b0 || same_x) && (sel_y == 1'b1 || same_y);
    assign sel_masks[3] = (sel_x == 1'b1 || same_x) && (sel_y == 1'b1 || same_y);
    wire [75:0] sel_next = {
        sel_last,        // 75
        sel_first,       // 74
        sel_masks,       // 73:70
        sel_key,         // 69:64
        sel_w3,          // 63:55
        sel_w2,          // 54:46
        sel_w1,          // 45:37
        sel_w0,          // 36:28
        header           // 27:0
    };
    reg [75:0] sel_e1;
    reg        v1;

    // E2 pack from the registered E1 selection.
    wire [8:0] p_w0 = sel_e1[36:28];
    wire [8:0] p_w1 = sel_e1[45:37];
    wire [8:0] p_w2 = sel_e1[54:46];
    wire [8:0] p_w3 = sel_e1[63:55];
    wire [5:0] p_key = sel_e1[69:64];
    wire [3:0] p_masks = sel_e1[73:70];
    wire [8:0] mw0 = p_masks[0] ? p_w0 : 9'd0;
    wire [8:0] mw1 = p_masks[1] ? p_w1 : 9'd0;
    wire [8:0] mw2 = p_masks[2] ? p_w2 : 9'd0;
    wire [8:0] mw3 = p_masks[3] ? p_w3 : 9'd0;
    wire [71:0] pack_next = {
        p_key[1:0],      // 71:70 lane
        p_key[5:2],      // 69:66 quad
        sel_e1[75],      // 65    last
        sel_e1[74],      // 64    first
        mw3, mw2, mw1, mw0,
        sel_e1[27:0]     // header
    };

    // E2..E7 align banks; E8 is the published output bank.
    reg [71:0] w0, w1, w2, w3, w4, w5;
    reg        v2, v3, v4, v5, v6, v7, v8;

    assign in_ready  = ce & ~fault;
    // Short alignment publishes the E2 pack register; the default keeps old E8.
    assign out_valid = SHORT_ALIGNMENT ? (v2 & ~fault) : (v8 & ~fault);

    always @(posedge clk) begin
        if (reset) begin
            fault      <= 1'b0;
            v0 <= 1'b0;
            v1 <= 1'b0;
            v2 <= 1'b0;
            v3 <= 1'b0;
            v4 <= 1'b0;
            v5 <= 1'b0;
            v6 <= 1'b0;
            v7 <= 1'b0;
            v8 <= 1'b0;
            out_packet <= 72'd0;
        end else begin
            if (do_fault) begin
                fault <= 1'b1;
            end
            if (advance) begin
                v0      <= in_valid;
                cap_e0  <= cap_next;
                v1      <= v0;
                sel_e1  <= sel_next;
                v2      <= v1;
                if (SHORT_ALIGNMENT) begin
                    // Publish the E2 pack word; the constant removes the tail.
                    out_packet <= pack_next;
                end else begin
                    w0      <= pack_next;
                    v3      <= v2;
                    w1      <= w0;
                    v4      <= v3;
                    w2      <= w1;
                    v5      <= v4;
                    w3      <= w2;
                    v6      <= v5;
                    w4      <= w3;
                    v7      <= v6;
                    w5      <= w4;
                    v8      <= v7;
                    out_packet <= w5;
                end
            end
        end
    end

endmodule
