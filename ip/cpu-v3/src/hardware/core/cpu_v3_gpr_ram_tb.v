module tb;
reg clk = 0;
reg write_enable = 0;
reg [3:0] write_address = 0;
reg [15:0] write_data = 0;
reg [3:0] read_a_address = 0;
reg [3:0] read_b_address = 0;
wire [15:0] read_a_data;
wire [15:0] read_b_data;

CpuV3GprRam dut(.*);
always #5 clk = ~clk;

initial begin
    read_a_address = 4'd3;
    read_b_address = 4'd9;
    #1;
    if (read_a_data !== 0 || read_b_data !== 0)
        $fatal(1, "register file did not power on zeroed");

    write_enable = 1;
    write_address = 4'd3;
    write_data = 16'h1234;
    @(posedge clk);
    #1;
    write_enable = 0;
    if (read_a_data !== 16'h1234 || read_b_data !== 0)
        $fatal(1, "first write or independent reads failed");

    write_enable = 1;
    write_address = 4'd9;
    write_data = 16'hbeef;
    #1;
    if (read_b_data !== 0)
        $fatal(1, "write became visible before the clock edge");
    @(posedge clk);
    #1;
    write_enable = 0;
    if (read_a_data !== 16'h1234 || read_b_data !== 16'hbeef)
        $fatal(1, "second write did not reach both read copies");

    read_a_address = 4'd9;
    read_b_address = 4'd3;
    #1;
    if (read_a_data !== 16'hbeef || read_b_data !== 16'h1234)
        $fatal(1, "read ports are not independently addressed");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
