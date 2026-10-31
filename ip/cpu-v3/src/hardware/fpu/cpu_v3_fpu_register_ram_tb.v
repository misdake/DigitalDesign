module tb;
reg clk = 0;
reg read_b_enable = 1;
reg write_enable = 0;
reg [8:0] write_address = 0;
reg [31:0] write_data = 0;
reg [8:0] read_a_address = 0;
reg [8:0] read_b_address = 0;
wire [31:0] read_a_data;
wire [31:0] read_b_data;

`ifdef CPU_V3_FPU_RF_EXEC_TEST
CpuV3FpuExecRf dut(.*);
localparam HOLD_B = 1;
`else
CpuV3FpuRegisterRam dut(.*);
localparam HOLD_B = 0;
`endif
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
    read_a_address = 9'd128;
    read_b_address = 9'd128;
    @(posedge clk);
    #1;
    check_value(read_a_data, 32'h04090000, "packed RCP interval 0");
    check_value(read_b_data, 32'h06030000, "packed RSQRT-even interval 0");

    // Address 256 is SINCOS interval 0 in mirror A and RSQRT-odd interval 0 in
    // mirror B, proving the asymmetric packed layout.
    @(negedge clk);
    read_a_address = 9'd256;
    read_b_address = 9'd256;
    @(posedge clk);
    #1;
    check_value(read_a_data, 32'h03240000, "packed SINCOS interval 0");
    check_value(read_b_data, 32'h0698B505, "packed RSQRT-odd interval 0");

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

    // Park B at a known word, then move both addresses and write while B
    // reads are disabled. Ordinary RF must still read; execution RF must hold.
    @(negedge clk);
    read_b_address = 9'd7;
    @(posedge clk);
    #1;
    expected_b = reference[7];
    check_value(read_b_data, expected_b, "B before hold");
    for (i = 0; i < 4; i = i + 1) begin
        @(negedge clk);
        read_b_enable = 0;
        read_b_address = 200 + i;
        read_a_address = read_b_address;
        write_enable = 1;
        write_address = read_b_address;
        write_data = 32'h1234_0000 + i;
        expected_a = reference[read_a_address];
        if (!HOLD_B) expected_b = reference[read_b_address];
        @(posedge clk);
        #1;
        check_value(read_a_data, expected_a, "A read-first during B hold");
        check_value(read_b_data, expected_b, "B hold while address/write change");
    end

    // A disabled read must not inhibit writes to either mirror, including
    // the held word itself. Resume on another same-address write: old data
    // must be read on the resume edge, then the new data on the next edge.
    @(negedge clk);
    read_a_address = 9'd7;
    read_b_address = 9'd7;
    write_address = 9'd7;
    write_data = 32'habcd_1234;
    expected_a = reference[7];
    if (!HOLD_B) expected_b = reference[7];
    @(posedge clk);
    #1;
    check_value(read_a_data, expected_a, "A while writing held B word");
    check_value(read_b_data, expected_b, "B held while writing held word");
    @(negedge clk);
    read_b_enable = 1;
    write_data = 32'hcafe_5678;
    @(posedge clk);
    #1;
    check_value(read_a_data, 32'habcd_1234, "A resume read-first");
    check_value(read_b_data, 32'habcd_1234, "B resume read-first");
    @(negedge clk);
    write_enable = 0;
    @(posedge clk);
    #1;
    check_value(read_a_data, 32'hcafe_5678, "A write during B hold committed");
    check_value(read_b_data, 32'hcafe_5678, "B write during B hold committed");

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
    expected_b = read_b_data;
    for (i = 0; i < 2000; i = i + 1) begin
        @(negedge clk);
        write_enable = $random(seed) & 1'b1;
        write_address = $random(seed);
        write_data = $random(seed);
        read_a_address = $random(seed);
        read_b_address = $random(seed);
        expected_a = reference[read_a_address];
        read_b_enable = $random(seed) & 1'b1;
        if (!HOLD_B || read_b_enable) expected_b = reference[read_b_address];
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
`ifdef CPU_V3_FPU_RF_TRACE
always @(posedge clk) begin
    #1;
    $display("RF_TRACE %0d %b %h %h %h %h %b %h %h", cycles,
        write_enable, write_address, write_data, read_a_address, read_b_address,
        read_b_enable, read_a_data, read_b_data);
end
`endif
endmodule
