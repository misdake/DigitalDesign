// Flash readback probe: reads the first bytes of the G16 boot package at
// 0x100000 through the fitted SPI flash reader and shows per-byte magic
// matches on the LEDs. LEDs 1..6 = bytes 0..5 match "G16BOOT\0"; all six lit
// means the package write landed.
module FlashReadbackProbe (
    input wire clk,
    input wire [1:0] buttons,
    input wire flash_miso,
    output wire [5:0] leds,
    output wire uart_tx,
    output wire flash_clk,
    output wire flash_cs_n,
    output wire flash_mosi
);

reg [3:0] startup_reset = 4'd15;
always @(posedge clk)
    if (startup_reset != 0)
        startup_reset <= startup_reset - 1'b1;
wire reset = startup_reset != 0;

reg start = 0;
wire flash_ready;
wire data_valid;
wire [7:0] data;
wire done;
wire error;

__FLASH_READER__ u_flash (
    .clk(clk),
    .start(start),
    .address(24'h100000),
    .length(24'd8),
    .data_ready(1'b1),
    .flash_miso(flash_miso),
    .ready(flash_ready),
    .data_valid(data_valid),
    .data(data),
    .done(done),
    .error(error),
    .flash_clk(flash_clk),
    .flash_cs_n(flash_cs_n),
    .flash_mosi(flash_mosi)
);

function [7:0] expected_byte;
    input [2:0] index;
    begin
        case (index)
            0: expected_byte = 8'h47; // G
            1: expected_byte = 8'h31; // 1
            2: expected_byte = 8'h36; // 6
            3: expected_byte = 8'h42; // B
            4: expected_byte = 8'h4f; // O
            5: expected_byte = 8'h4f; // O
            default: expected_byte = 8'h00;
        endcase
    end
endfunction

reg [5:0] matches = 0;
reg [2:0] byte_index = 0;
reg started = 0;
reg seen_done = 0;
reg seen_error = 0;

always @(posedge clk) begin
    if (reset) begin
        start <= 0;
        matches <= 0;
        byte_index <= 0;
        started <= 0;
        seen_done <= 0;
        seen_error <= 0;
    end else begin
        if (!started && flash_ready) begin
            start <= 1;
            started <= 1;
        end else begin
            start <= 0;
        end
        if (data_valid) begin
            if (byte_index < 6 && data == expected_byte(byte_index))
                matches[byte_index] <= 1;
            byte_index <= byte_index + 1'b1;
        end
        if (done)
            seen_done <= 1;
        if (error)
            seen_error <= 1;
    end
end

assign leds = {seen_error, seen_done, matches[3], matches[2], matches[1], matches[0]};
assign uart_tx = 1'b1;

endmodule
