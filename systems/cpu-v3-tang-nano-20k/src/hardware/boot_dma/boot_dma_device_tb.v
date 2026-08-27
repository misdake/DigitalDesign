module tb;
reg clk = 0;
reg reset = 0;
reg [2:0] device_index = 2;
reg [3:0] device_channel = 0;
reg device_read_enable = 0;
reg device_write_enable = 0;
reg [15:0] device_write_data = 0;
reg dma_busy = 0;
reg dma_done = 0;
reg dma_error = 0;
reg [7:0] dma_error_code = 0;
reg [31:0] dma_completed_words = 0;
wire [15:0] device_read_data;
wire dma_start;
wire [23:0] flash_offset;
wire [21:0] destination;
wire [31:0] file_size_bytes;
wire [31:0] memory_size_bytes;

BootDmaDevice dut(.*);
always #5 clk = ~clk;

task write_channel;
    input [3:0] channel;
    input [15:0] value;
    begin
        device_channel = channel;
        device_write_data = value;
        device_write_enable = 1;
        @(posedge clk);
        #1;
        device_write_enable = 0;
    end
endtask

initial begin
    write_channel(2, 16'hbcde);
    write_channel(3, 16'h007a);
    write_channel(4, 16'h4567);
    write_channel(5, 16'h0032);
    if (flash_offset !== 24'h7abcde || destination !== 22'h324567)
        $finish(1);

    write_channel(0, 1);
    if (!dma_start) $finish(1);
    @(posedge clk);
    #1;
    if (dma_start) $finish(1);

    dma_error = 1;
    dma_error_code = 3;
    device_read_enable = 1;
    device_channel = 1;
    #1;
    if (device_read_data !== 16'h8000) $finish(1);
    device_channel = 14;
    #1;
    if (device_read_data !== 3) $finish(1);
    device_index = 3;
    #1;
    if (device_read_data !== 0) $finish(1);

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #1000;
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
