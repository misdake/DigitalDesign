module tb;
reg clk = 0;
reg write_enable = 0;
reg [5:0] write_address = 0;
reg [15:0] write_data = 0;
reg [5:0] read_a_address = 0;
reg [5:0] read_b_address = 0;
wire [15:0] read_a_data;
wire [15:0] read_b_data;

CpuV3FpuRegisterRam dut(.*);
always #5 clk = ~clk;

initial begin
    // Power-on state is zero on both read ports.
    read_a_address = 6'd9;
    read_b_address = 6'd9;
    #1;
    if (read_a_data !== 16'd0 || read_b_data !== 16'd0)
        $fatal(1, "register file did not power on zeroed");

    // A write is visible on both asynchronous read ports after the edge.
    write_address = 6'd9;
    write_data = 16'hbeef;
    write_enable = 1;
    @(posedge clk);
    #1;
    write_enable = 0;
    if (read_a_data !== 16'hbeef || read_b_data !== 16'hbeef)
        $fatal(1, "write not visible on both read ports");

    // The two read ports are independent.
    write_address = 6'd10;
    write_data = 16'h1234;
    write_enable = 1;
    @(posedge clk);
    #1;
    write_enable = 0;
    read_b_address = 6'd10;
    #1;
    if (read_a_data !== 16'hbeef || read_b_data !== 16'h1234)
        $fatal(1, "independent read ports failed");

    // A read of the written address in the write cycle still returns the old
    // value until the clock edge.
    write_address = 6'd10;
    write_data = 16'h4321;
    write_enable = 1;
    #1;
    if (read_b_data !== 16'h1234)
        $fatal(1, "read during write did not return the old value");
    @(posedge clk);
    #1;
    write_enable = 0;
    if (read_b_data !== 16'h4321)
        $fatal(1, "write did not land on the clock edge");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
