// Testbench for CpuV3FpuV2MulPipe: directed corners plus randomized streams
// against a 64-bit reference with a fixed three-cycle latency.
module tb;
reg clk = 0;
always #5 clk = ~clk;

reg abort = 0;
reg in_valid = 0;
reg [31:0] in_a = 0;
reg [31:0] in_b = 0;
reg [8:0] in_tag = 0;
wire out_valid;
wire signed [63:0] out_product;
wire [8:0] out_tag;

CpuV3FpuV2MulPipe dut (
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

// Reference model: a three-entry delay line of (a*b, tag), one entry per
// cycle, wrap-free in 64 bits. out_valid must be high exactly when the third
// stage holds a live entry.
reg signed [63:0] ref_a [0:2];
reg signed [63:0] ref_b [0:2];
reg [8:0] ref_tag [0:2];
reg ref_valid [0:2];
reg signed [63:0] ref_product;
integer s;

// The check runs one beat after the push's capture edge, so the output ports
// show the entry driven two pushes ago: ref index 1.
task check_output;
    begin
        if (out_valid !== ref_valid[1]) begin
            $display("DIGITAL_DESIGN_FAIL: out_valid=%b want %b", out_valid, ref_valid[1]);
            $finish;
        end
        if (out_valid) begin
            check_count = check_count + 1;
            ref_product = ref_a[1] * ref_b[1];
            if (out_product !== ref_product || out_tag !== ref_tag[1]) begin
                $display("DIGITAL_DESIGN_FAIL: got product=%h tag=%0d want %h %0d",
                    out_product, out_tag, ref_product, ref_tag[1]);
                $finish;
            end
        end
    end
endtask

// Advance the reference delay line after the DUT's edge semantics.
task push;
    input v;
    input [31:0] a;
    input [31:0] b;
    input [8:0] tg;
    begin
        @(negedge clk);
        in_valid = v; in_a = a; in_b = b; in_tag = tg;
        @(posedge clk); #1;
        check_output;
        ref_valid[2] = ref_valid[1]; ref_valid[1] = ref_valid[0]; ref_valid[0] = v;
        ref_a[2] = ref_a[1]; ref_a[1] = ref_a[0];
        ref_b[2] = ref_b[1]; ref_b[1] = ref_b[0];
        ref_tag[2] = ref_tag[1]; ref_tag[1] = ref_tag[0];
        if (v) begin
            ref_a[0] = {{32{a[31]}}, a};
            ref_b[0] = {{32{b[31]}}, b};
            ref_tag[0] = tg;
        end
    end
endtask

initial begin
    ref_valid[0] = 0; ref_valid[1] = 0; ref_valid[2] = 0;
    ref_a[0] = 0; ref_a[1] = 0; ref_a[2] = 0;
    ref_b[0] = 0; ref_b[1] = 0; ref_b[2] = 0;
    ref_tag[0] = 0; ref_tag[1] = 0; ref_tag[2] = 0;

    // Directed corners: 1.0*1.0, signs, fraction, wrap at both extremes.
    push(1, 32'h00010000, 32'h00010000, 9'd5);   // 1.0 * 1.0
    push(1, 32'hffff0000, 32'h00020000, 9'd6);   // -1.0 * 2.0
    push(1, 32'h00008000, 32'h00004000, 9'd7);   // 0.5 * 0.25
    push(1, 32'h80000000, 32'h80000000, 9'd8);   // min * min (wrap)
    push(1, 32'h7fffffff, 32'h7fffffff, 9'd9);   // max * max
    push(0, 0, 0, 0);
    push(0, 0, 0, 0);
    push(0, 0, 0, 0);
    push(0, 0, 0, 0);

    // abort mid-stream: the two in-flight entries are voided; afterwards the
    // pipe must stay empty for the whole drain window.
    push(1, 32'h00030000, 32'h00040000, 9'd10);
    push(1, 32'h00050000, 32'h00060000, 9'd11);
    @(negedge clk); in_valid = 0;   // stop driving before the abort
    @(negedge clk); abort = 1;
    @(posedge clk);            // abort kills both in-flight entries here
    @(negedge clk); abort = 0;
    ref_valid[0] = 0; ref_valid[1] = 0; ref_valid[2] = 0;
    ref_a[0] = 0; ref_a[1] = 0; ref_a[2] = 0;
    ref_b[0] = 0; ref_b[1] = 0; ref_b[2] = 0;
    // The abort posedge killed both entries; the next five beats must show no
    // output at all (checked by hand, not via the delay line).
    @(negedge clk); in_valid = 0;
    for (i = 0; i < 5; i = i + 1) begin
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
