module tb;
reg clk = 0;
reg clear_enable = 0;
reg [5:0] clear_set = 0;
reg write_enable = 0;
reg write_way = 0;
reg [5:0] write_set = 0;
reg write_value = 0;
reg victim_write_enable = 0;
reg victim_write_value = 0;
reg [5:0] read_set = 0;
wire way_0_valid;
wire way_1_valid;
wire victim;

CpuV3CacheValidRam dut(.*);
always #5 clk = ~clk;

integer sweep;
initial begin
    // Image-independent start: sweep every set of both ways to zero.
    clear_enable = 1;
    for (sweep = 0; sweep < 64; sweep = sweep + 1) begin
        clear_set = sweep[5:0];
        @(posedge clk);
    end
    clear_enable = 0;

    write_enable = 1;
    write_way = 0;
    write_set = 6'd3;
    write_value = 1;
    @(posedge clk);
    #1;
    write_enable = 0;
    read_set = 6'd3;
    #1;
    if (!way_0_valid || way_1_valid)
        $fatal(1, "way 0 valid write/read failed");

    read_set = 6'd4;
    #1;
    if (way_0_valid || way_1_valid)
        $fatal(1, "asynchronous read of an unwritten set failed");

    write_enable = 1;
    write_way = 1;
    write_value = 1;
    victim_write_enable = 1;
    victim_write_value = 0;
    @(posedge clk);
    #1;
    write_enable = 0;
    victim_write_enable = 0;
    read_set = 6'd3;
    #1;
    if (!way_0_valid || !way_1_valid || victim != 0)
        $fatal(1, "way 1 valid/victim write failed");

    clear_enable = 1;
    clear_set = 6'd3;
    @(posedge clk);
    #1;
    clear_enable = 0;
    #1;
    if (way_0_valid || way_1_valid)
        $fatal(1, "sweep clear did not invalidate both ways");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
