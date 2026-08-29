module tb;
reg clk = 0;
reg [9:0] read_address = 0;
reg even_write_enable = 0;
reg odd_write_enable = 0;
reg [9:0] write_address = 0;
reg [15:0] even_write_data = 0;
reg [15:0] odd_write_data = 0;
wire [15:0] even_read_data;
wire [15:0] odd_read_data;
integer cycles = 0;

CpuV3ParitySplitCacheData dut(.*);
always #5 clk = ~clk;

always @(posedge clk) begin
    cycles <= cycles + 1;
    if (cycles > 100)
        $fatal(1, "testbench cycle limit exceeded");
end

initial begin
    @(posedge clk);
    write_address <= 10'd37;
    even_write_data <= 16'h1357;
    odd_write_data <= 16'h2468;
    even_write_enable <= 1;
    odd_write_enable <= 1;
    @(posedge clk);
    even_write_enable <= 0;
    odd_write_enable <= 0;
    read_address <= 10'd37;
    @(posedge clk);
    #1;
    if (even_read_data != 16'h1357 || odd_read_data != 16'h2468)
        $fatal(1, "parity banks did not preserve simultaneous independent writes");

    odd_write_data <= 16'habcd;
    odd_write_enable <= 1;
    @(posedge clk);
    odd_write_enable <= 0;
    @(posedge clk);
    #1;
    if (even_read_data != 16'h1357 || odd_read_data != 16'habcd)
        $fatal(1, "odd-only write modified the even bank");

    even_write_data <= 16'hef01;
    even_write_enable <= 1;
    @(posedge clk);
    even_write_enable <= 0;
    @(posedge clk);
    #1;
    if (even_read_data != 16'hef01 || odd_read_data != 16'habcd)
        $fatal(1, "even-only write modified the odd bank");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
