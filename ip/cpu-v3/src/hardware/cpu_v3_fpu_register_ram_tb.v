module tb;
reg clk = 0;
reg [3:0] write_enable = 0;
reg [3:0] write_address = 0;
reg [63:0] write_data = 0;
reg [3:0] read_a_address = 0;
reg [3:0] read_b_address = 0;
wire [63:0] read_a_data;
wire [63:0] read_b_data;

CpuV3FpuRegisterRam dut(.*);
always #5 clk = ~clk;

initial begin
    // Power-on state is zero on both read ports.
    read_a_address = 4'd9;
    read_b_address = 4'd9;
    #1;
    if (read_a_data !== 64'd0 || read_b_data !== 64'd0)
        $fatal(1, "register file did not power on zeroed");

    // A full-vector write is visible on both asynchronous read ports after
    // the edge.
    write_address = 4'd9;
    write_data = 64'hdead_beef_1234_5678;
    write_enable = 4'b1111;
    @(posedge clk);
    #1;
    write_enable = 0;
    if (read_a_data !== 64'hdead_beef_1234_5678 || read_b_data !== 64'hdead_beef_1234_5678)
        $fatal(1, "write not visible on both read ports");

    // The two read ports are independent.
    write_address = 4'd10;
    write_data = 64'h0000_0000_0000_1234;
    write_enable = 4'b1111;
    @(posedge clk);
    #1;
    write_enable = 0;
    read_b_address = 4'd10;
    #1;
    if (read_a_data !== 64'hdead_beef_1234_5678 || read_b_data !== 64'h1234)
        $fatal(1, "independent read ports failed");

    // A single-lane write updates only that lane and preserves the rest.
    write_address = 4'd9;
    write_data = 64'h0000_cafe_0000_0000;
    write_enable = 4'b0100;
    @(posedge clk);
    #1;
    write_enable = 0;
    if (read_a_data !== 64'hdead_cafe_1234_5678)
        $fatal(1, "per-lane write enable leaked into other lanes");

    // A read of the written address in the write cycle still returns the old
    // value until the clock edge.
    write_address = 4'd10;
    write_data = 64'h0000_0000_0000_4321;
    write_enable = 4'b0001;
    #1;
    if (read_b_data !== 64'h1234)
        $fatal(1, "read during write did not return the old value");
    @(posedge clk);
    #1;
    write_enable = 0;
    if (read_b_data !== 64'h4321)
        $fatal(1, "lane write did not land on the clock edge");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
