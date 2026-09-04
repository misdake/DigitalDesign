module tb;
reg clk = 0;
reg bank_0_a_write_enable = 0;
reg [9:0] bank_0_a_address = 0;
reg [15:0] bank_0_a_write_data = 0;
wire [15:0] bank_0_a_read_data;
reg bank_0_b_write_enable = 0;
reg [9:0] bank_0_b_address = 0;
reg [15:0] bank_0_b_write_data = 0;
wire [15:0] bank_0_b_read_data;
reg bank_1_a_write_enable = 0;
reg [9:0] bank_1_a_address = 0;
reg [15:0] bank_1_a_write_data = 0;
wire [15:0] bank_1_a_read_data;
reg bank_1_b_write_enable = 0;
reg [9:0] bank_1_b_address = 0;
reg [15:0] bank_1_b_write_data = 0;
wire [15:0] bank_1_b_read_data;
integer cycles = 0;

CpuV3DualPortCacheData dut(.*);
always #5 clk = ~clk;

always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > 100)
        $fatal(1, "testbench cycle limit exceeded");
end

initial begin
    // One refill beat writes four consecutive words through all four ports.
    @(negedge clk);
    bank_0_a_address = {1'b0, 6'd7, 2'd2, 1'b0};
    bank_0_b_address = {1'b0, 6'd7, 2'd2, 1'b1};
    bank_1_a_address = {1'b0, 6'd7, 2'd2, 1'b0};
    bank_1_b_address = {1'b0, 6'd7, 2'd2, 1'b1};
    bank_0_a_write_data = 16'h1000;
    bank_1_a_write_data = 16'h1001;
    bank_0_b_write_data = 16'h1002;
    bank_1_b_write_data = 16'h1003;
    bank_0_a_write_enable = 1;
    bank_0_b_write_enable = 1;
    bank_1_a_write_enable = 1;
    bank_1_b_write_enable = 1;
    @(negedge clk);
    bank_0_a_write_enable = 0;
    bank_0_b_write_enable = 0;
    bank_1_a_write_enable = 0;
    bank_1_b_write_enable = 0;

    // The two ports of one parity bank can read the same word from both ways.
    bank_0_a_address = {1'b0, 6'd7, 3'd4};
    bank_0_b_address = {1'b1, 6'd7, 3'd4};
    bank_1_a_address = {1'b0, 6'd7, 3'd4};
    bank_1_b_address = {1'b1, 6'd7, 3'd4};
    @(negedge clk);
    if (bank_0_a_read_data != 16'h1000 ||
        bank_1_a_read_data != 16'h1001)
        $fatal(1, "way-zero lookup data mismatch");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
