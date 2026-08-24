module tb;
reg clk = 0;
reg reset = 1;
reg start = 0;
reg [23:0] flash_offset = 0;
reg [21:0] destination = 0;
reg [31:0] file_size_bytes = 0;
reg [31:0] memory_size_bytes = 0;
reg [31:0] expected_crc32 = 0;
reg flash_miso = 0;
reg [31:0] sdram_read_data = 0;
reg sdram_read_valid = 0;
reg sdram_init_done = 0;
reg sdram_command_ack = 0;
wire busy;
wire done;
wire error;
wire [7:0] error_code;
wire [31:0] actual_crc32;
wire [31:0] completed_words;
wire flash_clk;
wire flash_cs_n;
wire flash_mosi;
wire sdram_command_valid;
wire [2:0] sdram_command;
wire sdram_precharge;
wire [20:0] sdram_address;
wire [3:0] sdram_write_mask;
wire [31:0] sdram_write_data;
wire [7:0] sdram_burst_length;

TangNano20KBootDma dut(.*);
always #5 clk = ~clk;

initial begin
    repeat (2) @(posedge clk);
    reset = 0;
    @(posedge clk);
    #1;
    if (busy || done || error || !flash_cs_n || sdram_command_valid) begin
        $display("FAIL: target boot DMA did not reset idle");
        $finish(1);
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    #1000;
    $display("FAIL: timeout");
    $finish(1);
end
endmodule
