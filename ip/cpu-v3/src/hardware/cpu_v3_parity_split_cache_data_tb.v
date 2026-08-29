module tb;
reg clk = 0;
reg [9:0] bank_0_read_address = 0;
reg [9:0] bank_1_read_address = 0;
reg bank_0_write_enable = 0;
reg bank_1_write_enable = 0;
reg [9:0] write_address = 0;
reg [15:0] bank_0_write_data = 0;
reg [15:0] bank_1_write_data = 0;
wire [15:0] bank_0_read_data;
wire [15:0] bank_1_read_data;
integer cycles = 0;

CpuV3ParitySplitCacheData dut(.*);
always #5 clk = ~clk;

always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > 100)
        $fatal(1, "testbench cycle limit exceeded");
end

initial begin
    // way 0: bank 0 holds the even word and bank 1 the odd word.
    @(posedge clk);
    write_address <= {1'b0, 9'd37};
    bank_0_write_data <= 16'h1357;
    bank_1_write_data <= 16'h2468;
    bank_0_write_enable <= 1;
    bank_1_write_enable <= 1;
    @(posedge clk);
    bank_0_write_enable <= 0;
    bank_1_write_enable <= 0;

    // way 1 reverses parity between the banks.
    write_address <= {1'b1, 9'd37};
    bank_0_write_data <= 16'h9abc;
    bank_1_write_data <= 16'h5678;
    bank_0_write_enable <= 1;
    bank_1_write_enable <= 1;
    @(posedge clk);
    bank_0_write_enable <= 0;
    bank_1_write_enable <= 0;

    // An even-word lookup reads way 0 from bank 0 and way 1 from bank 1.
    bank_0_read_address <= {1'b0, 9'd37};
    bank_1_read_address <= {1'b1, 9'd37};
    @(posedge clk);
    #1;
    if (bank_0_read_data != 16'h1357 || bank_1_read_data != 16'h5678)
        $fatal(1, "even lookup did not return both ways in one cycle");

    // An odd-word lookup reverses the two read addresses.
    bank_0_read_address <= {1'b1, 9'd37};
    bank_1_read_address <= {1'b0, 9'd37};
    @(posedge clk);
    #1;
    if (bank_0_read_data != 16'h9abc || bank_1_read_data != 16'h2468)
        $fatal(1, "odd lookup did not return both ways in one cycle");

    // A hit write touches only its selected bank.
    write_address <= {1'b1, 9'd37};
    bank_0_write_data <= 16'hef01;
    bank_0_write_enable <= 1;
    @(posedge clk);
    bank_0_write_enable <= 0;
    @(posedge clk);
    #1;
    if (bank_0_read_data != 16'hef01 || bank_1_read_data != 16'h2468)
        $fatal(1, "single-bank write corrupted the other bank");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
