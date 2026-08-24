module tb;
reg clk = 0;
reg [1:0] buttons = 0;
reg flash_miso = 0;
reg [31:0] sdram_read_data = 0;
reg sdram_read_valid = 0;
reg sdram_init_done = 0;
reg sdram_command_ack = 0;
wire [5:0] leds;
wire uart_tx;
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

BootDmaSelfTest dut(.*);
always #5 clk = ~clk;

initial begin
    repeat (3) @(posedge clk);
    if (!flash_cs_n || !uart_tx) $finish(1);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
