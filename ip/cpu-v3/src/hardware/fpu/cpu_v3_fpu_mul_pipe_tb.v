// Testbench for CpuV3FpuMulPipe: directed corners plus randomized streams
// against a 64-bit reference with a fixed two-cycle latency.
module tb;
reg clk = 0;
always #5 clk = ~clk;

reg abort = 0;
reg in_valid = 0;
// The pipe's operand buses are signed 36 bits; the TB drives the same
// sign-extended values the MUL/DOT owners present and SINCOS's positive K.
reg [35:0] in_a = 0;
reg [35:0] in_b = 0;
reg [8:0] in_tag = 0;
wire out_valid;
wire signed [63:0] out_product;
wire [8:0] out_tag;

CpuV3FpuMulPipe dut (
    .clk(clk), .abort(abort),
    .in_valid(in_valid), .in_a(in_a), .in_b(in_b), .in_tag(in_tag),
    .out_valid(out_valid), .out_product(out_product), .out_tag(out_tag)
);

localparam integer MAX_CYCLES = 20000;
integer cycles = 0;
always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > MAX_CYCLES) begin
        $display("DIGITAL_DESIGN_FAIL: cycle limit exceeded");
        $finish;
    end
end

integer check_count = 0;
integer i;
integer seed = 32'h5EED1234;

// After the second capture edge, the output must equal the previous push.
// Keep that transaction independently of the DUT's internal pipeline stages.
reg signed [63:0] ref_a = 0;
reg signed [63:0] ref_b = 0;
reg [8:0] ref_tag = 0;
reg ref_valid = 0;
reg signed [63:0] ref_product;

// Check after the current capture edge, before advancing the reference.
task check_output;
    begin
        if (out_valid !== ref_valid) begin
            $display("DIGITAL_DESIGN_FAIL: out_valid=%b want %b", out_valid, ref_valid);
            $finish;
        end
        if (out_valid) begin
            check_count = check_count + 1;
            ref_product = ref_a * ref_b;
            if (out_product !== ref_product || out_tag !== ref_tag) begin
                $display("DIGITAL_DESIGN_FAIL: got product=%h tag=%0d want %h %0d",
                    out_product, out_tag, ref_product, ref_tag);
                $finish;
            end
        end
    end
endtask

// Advance the reference delay line after the DUT's edge semantics.
task push_wide;
    input v;
    input [35:0] a;
    input [35:0] b;
    input [8:0] tg;
    begin
        @(negedge clk);
        in_valid = v;
        in_a = a;
        in_b = b;
        in_tag = tg;
        @(posedge clk); #1;
        check_output;
        ref_valid = v;
        if (v) begin
            ref_a = {{28{a[35]}}, a};
            ref_b = {{28{b[35]}}, b};
            ref_tag = tg;
        end
    end
endtask

task push;
    input v;
    input [31:0] a;
    input [31:0] b;
    input [8:0] tg;
    begin
        push_wide(v, {{4{a[31]}}, a}, {{4{b[31]}}, b}, tg);
    end
endtask

initial begin
    // Directed corners: 1.0*1.0, signs, fraction, wrap at both extremes.
    push(1, 32'h00010000, 32'h00010000, 9'd5);   // 1.0 * 1.0
    push(1, 32'hffff0000, 32'h00020000, 9'd6);   // -1.0 * 2.0
    push(1, 32'h00008000, 32'h00004000, 9'd7);   // 0.5 * 0.25
    push(1, 32'h80000000, 32'h80000000, 9'd8);   // min * min (wrap)
    push(1, 32'h7fffffff, 32'h7fffffff, 9'd9);   // max * max
    // SINCOS K has bit 31 set but is positive on its signed-36 bus.
    push_wide(1, 36'h0_7fffffff, 36'h0_a2f9836e, 9'd20);
    push_wide(1, 36'hf_80000000, 36'h0_a2f9836e, 9'd21);
    push_wide(1, 36'hf_ffffffff, 36'h0_a2f9836e, 9'd22);
    push(0, 0, 0, 0);
    push(0, 0, 0, 0);
    push(0, 0, 0, 0);
    push(0, 0, 0, 0);

    // Abort with both stages occupied: cancel the visible return immediately
    // and discard the pending transaction at the next edge.
    push(1, 32'h00030000, 32'h00040000, 9'd10);
    push(1, 32'h00050000, 32'h00060000, 9'd11);
    @(negedge clk); in_valid = 0; abort = 1;
    #1;
    if (out_valid !== 1'b0) begin
        $display("DIGITAL_DESIGN_FAIL: abort did not gate the visible return");
        $finish;
    end
    check_count = check_count + 1;
    @(posedge clk); #1;
    if (out_valid !== 1'b0) begin
        $display("DIGITAL_DESIGN_FAIL: abort did not clear the pipeline");
        $finish;
    end
    check_count = check_count + 1;
    @(negedge clk); abort = 0;
    ref_valid = 0;
    // The abort edge killed both entries; the next four beats must show no
    // output at all (checked by hand, not via the delay line).
    for (i = 0; i < 4; i = i + 1) begin
        @(posedge clk); #1;
        check_count = check_count + 1;
        if (out_valid !== 1'b0) begin
            $display("DIGITAL_DESIGN_FAIL: out_valid stuck after abort");
            $finish;
        end
    end

    // Random back-to-back stream.
    for (i = 0; i < 3000; i = i + 1)
        push($random(seed) & 1, $random(seed), $random(seed), $random(seed) & 9'h1ff);
    for (i = 0; i < 4; i = i + 1)
        push(0, 0, 0, 0);

    if (check_count == 0) begin
        $display("DIGITAL_DESIGN_FAIL: no checks executed");
        $finish;
    end
    $display("checks=%0d", check_count);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
