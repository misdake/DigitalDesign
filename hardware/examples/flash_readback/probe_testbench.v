// Testbench for FlashReadbackProbe: models the SPI flash with the package
// magic at 0x100000 and expects done plus all four match LEDs.
module tb;
reg clk = 0;
wire [5:0] leds;
wire uart_tx;
wire flash_clk;
wire flash_cs_n;
wire flash_mosi;
reg flash_miso = 1;

FlashReadbackProbe dut (
    .clk(clk),
    .buttons(2'b00),
    .flash_miso(flash_miso),
    .leds(leds),
    .uart_tx(uart_tx),
    .flash_clk(flash_clk),
    .flash_cs_n(flash_cs_n),
    .flash_mosi(flash_mosi)
);

always #5 clk = ~clk;

// SPI flash read model (same shape as the g16_boot harness): 03h command and
// a 24-bit address, then stream bytes from the image, 0xff beyond it.
localparam [23:0] FLASH_BASE = 24'h100000;
reg [7:0] flash_image [0:7];
reg [31:0] flash_command = 0;
integer flash_command_bits = 0;
reg [23:0] flash_byte_address = 0;
integer flash_data_bit = 0;
reg [7:0] flash_current_byte = 0;

integer flash_init_index;
initial begin
    flash_image[0] = 8'h47; // G
    flash_image[1] = 8'h31; // 1
    flash_image[2] = 8'h36; // 6
    flash_image[3] = 8'h42; // B
    flash_image[4] = 8'h4f; // O
    flash_image[5] = 8'h4f; // O
    flash_image[6] = 8'h54; // T
    flash_image[7] = 8'h00;
end

always @(posedge flash_cs_n) begin
    flash_command_bits = 0;
    flash_data_bit = 0;
end

always @(posedge flash_clk) begin
    if (!flash_cs_n && flash_command_bits < 32) begin
        flash_command = {flash_command[30:0], flash_mosi};
        flash_command_bits = flash_command_bits + 1;
        // The 24-bit address is complete after the 32nd command bit.
        if (flash_command_bits == 32)
            flash_byte_address = flash_command[23:0];
    end
end

always @(negedge flash_clk) begin
    if (!flash_cs_n && flash_command_bits >= 32) begin
        if (flash_data_bit == 0) begin
            if (flash_byte_address >= FLASH_BASE && flash_byte_address < FLASH_BASE + 8)
                flash_current_byte = flash_image[flash_byte_address - FLASH_BASE];
            else
                flash_current_byte = 8'hff;
            flash_byte_address = flash_byte_address + 1;
        end
        flash_miso <= flash_current_byte[7 - flash_data_bit];
        flash_data_bit = (flash_data_bit + 1) % 8;
    end
end

initial begin
    wait (leds[4] == 1'b1); // done
    repeat (10) @(posedge clk);
    if (leds !== 6'b011111) begin
        $display("FAIL: leds %b (want done + matches for bytes 0..3)", leds);
        $finish(1);
    end
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (500000) @(posedge clk);
    $display("FAIL: timeout, leds %b", leds);
    $finish(1);
end
endmodule
