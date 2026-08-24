module tb;
reg clk = 0;
reg write_enable = 0;
reg [5:0] address = 0;
reg [11:0] write_data = 0;
wire [11:0] read_data;

G16CacheTagRam dut(.*);
always #5 clk = ~clk;

initial begin
    address = 6'd7;
    write_data = 12'habc;
    write_enable = 1;
    @(posedge clk);
    write_enable = 0;
    #1;
    if (read_data != 12'habc)
        $fatal(1, "tag write/read failed");
    address = 6'd8;
    write_data = 12'h123;
    write_enable = 1;
    @(posedge clk);
    write_enable = 0;
    #1;
    if (read_data != 12'h123)
        $fatal(1, "second tag write/read failed");
    address = 6'd7;
    #1;
    if (read_data != 12'habc)
        $fatal(1, "asynchronous tag read failed");
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
