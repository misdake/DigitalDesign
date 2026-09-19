module tb;
reg clk = 0;
reg write_enable = 0;
reg [8:0] write_address = 0;
reg [31:0] write_data = 0;
reg [8:0] read_a_address = 0;
reg [8:0] read_b_address = 0;
wire [31:0] read_a_data;
wire [31:0] read_b_data;

CpuV3FpuRegisterRam dut(.*);
always #5 clk = ~clk;

localparam integer MAX_CYCLES = 20000;
integer cycles = 0;
always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > MAX_CYCLES) begin
        $display("DIGITAL_DESIGN_FAIL: cycle limit exceeded");
        $finish;
    end
end

// Reference model. A write commits one cycle after the edge, so a read that is
// issued in the same cycle as a write always observes the old word.
reg [31:0] reference [0:511];
integer check_count = 0;
integer i;
integer seed = 32'h1234_5678;
reg [31:0] expected_a;
reg [31:0] expected_b;

function [31:0] pattern;
    input [8:0] index;
    begin
        pattern = {index, 23'h5A5A5A};
    end
endfunction

task check_value;
    input [31:0] got;
    input [31:0] want;
    input [8*48-1:0] label;
    begin
        check_count = check_count + 1;
        if (got !== want) begin
            $display("DIGITAL_DESIGN_FAIL: %0s got %08h want %08h",
                label, got, want);
            $finish;
        end
    end
endtask

always @(posedge clk) begin
    if (write_enable)
        reference[write_address] <= write_data;
end

initial begin
    for (i = 0; i < 512; i = i + 1)
        reference[i] = 0;

    // Hidden LUT region: the register file powers up with the special-function
    // tables already in BSRAM. Check the two table bases plus the mirror
    // asymmetry before the writes below overwrite everything. Reads have one
    // cycle of latency, so drive the addresses on the falling edge and sample
    // after the next rising edge.
    @(negedge clk);
    read_a_address = 9'd64;
    read_b_address = 9'd64;
    @(posedge clk);
    #1;
    check_value(read_a_data, 32'h00010000, "LUT mirror_0[64] RCP(1.0)");
    check_value(read_b_data, 32'h00010000, "LUT mirror_1[64] RSQRT(1.0)");

    // mirror_0[192] is SINCOS sin(0) = 0; mirror_1[192] is an RSQRT entry and
    // must be nonzero, proving the two mirrors really carry different tables.
    @(negedge clk);
    read_a_address = 9'd192;
    read_b_address = 9'd192;
    @(posedge clk);
    #1;
    check_value(read_a_data, 32'h0, "LUT mirror_0[192] SINCOS sin(0)");
    if (read_b_data === 32'h0) begin
        $display("DIGITAL_DESIGN_FAIL: LUT mirror_1[192] RSQRT entry is zero");
        $finish;
    end
    check_count = check_count + 1;

    // Write every physical address with a distinct word.
    write_enable = 0;
    for (i = 0; i < 512; i = i + 1) begin
        @(negedge clk);
        write_enable = 1;
        write_address = i[8:0];
        write_data = pattern(i[8:0]);
    end
    @(negedge clk);
    write_enable = 0;

    // Read the whole array back through port A.
    for (i = 0; i < 512; i = i + 1) begin
        @(negedge clk);
        read_a_address = i[8:0];
        @(posedge clk);
        #1;
        check_value(read_a_data, pattern(i[8:0]), "port A full-array readback");
    end

    // Read the whole array back through port B.
    for (i = 0; i < 512; i = i + 1) begin
        @(negedge clk);
        read_b_address = i[8:0];
        @(posedge clk);
        #1;
        check_value(read_b_data, pattern(i[8:0]), "port B full-array readback");
    end

    // A write and a read of the same address in one cycle must still present
    // the old word on both read ports.
    @(negedge clk);
    write_enable = 1;
    write_address = 9'd321;
    write_data = 32'hdead_beef;
    read_a_address = 9'd321;
    read_b_address = 9'd321;
    expected_a = reference[321];
    expected_b = reference[321];
    @(posedge clk);
    #1;
    check_value(read_a_data, expected_a, "same-cycle write/read port A old data");
    check_value(read_b_data, expected_b, "same-cycle write/read port B old data");

    // The write lands on the following edge, and both ports agree.
    @(negedge clk);
    write_enable = 0;
    read_a_address = 9'd321;
    read_b_address = 9'd321;
    @(posedge clk);
    #1;
    check_value(read_a_data, 32'hdead_beef, "write landed for port A");
    check_value(read_b_data, 32'hdead_beef, "write landed for port B");
    check_value(read_a_data, read_b_data, "A and B agree at one address");

    // Randomized 2R1W traffic. Inputs are driven on the falling edge and the
    // registered read outputs are compared against the reference model that
    // still holds the pre-write contents.
    for (i = 0; i < 2000; i = i + 1) begin
        @(negedge clk);
        write_enable = $random(seed) & 1'b1;
        write_address = $random(seed);
        write_data = $random(seed);
        read_a_address = $random(seed);
        read_b_address = $random(seed);
        expected_a = reference[read_a_address];
        expected_b = reference[read_b_address];
        @(posedge clk);
        #1;
        check_value(read_a_data, expected_a, "random port A old data");
        check_value(read_b_data, expected_b, "random port B old data");
    end

    if (check_count == 0) begin
        $display("DIGITAL_DESIGN_FAIL: no checks executed");
        $finish;
    end
    $display("checks=%0d", check_count);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
