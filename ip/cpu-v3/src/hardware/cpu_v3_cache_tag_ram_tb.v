module tb;
reg clk = 0;
reg write_enable = 0;
reg write_way = 0;
reg [5:0] address = 0;
reg [11:0] write_data = 0;
wire [11:0] way_0_read_data;
wire [11:0] way_1_read_data;

CpuV3CacheTagRam dut(.*);
always #5 clk = ~clk;

initial begin
    address = 6'd7;
    write_data = 12'habc;
    write_enable = 1;
    @(posedge clk);
    write_enable = 0;
    #1;
    if (way_0_read_data != 12'habc || way_1_read_data != 0)
        $fatal(1, "way 0 tag write/read failed");
    write_way = 1;
    write_data = 12'h123;
    write_enable = 1;
    @(posedge clk);
    write_enable = 0;
    #1;
    if (way_0_read_data != 12'habc || way_1_read_data != 12'h123)
        $fatal(1, "way 1 tag write corrupted way 0");
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
