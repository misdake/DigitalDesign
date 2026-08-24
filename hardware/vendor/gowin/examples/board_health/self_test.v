// Minimal, self-contained Tang Nano 20K health probe. This module must remain
// independent of CPU, memory, PLL, and system-control RTL: it is the gate that
// proves the board clock, user inputs, FPGA fabric, UART pin, and host capture
// path before a higher-level design is interpreted.
module BoardHealthProbe (
    input wire clk,
    input wire [1:0] buttons,
    output wire [5:0] leds,
    output wire uart_tx
);

reg [1:0] button_meta = 0;
reg [1:0] button_sync = 0;
reg [31:0] heartbeat = 0;
always @(posedge clk) begin
    button_meta <= buttons;
    button_sync <= button_meta;
    heartbeat <= heartbeat + 1'b1;
end

wire uart_busy;
wire frame_toggle;
__DIAGNOSTIC_REPORTER__ u_reporter(
    .clk(clk),
    .report_enable(1'b1),
    .status({6'b0, button_sync}),
    .uart_tx(uart_tx),
    .uart_busy(uart_busy),
    .frame_toggle(frame_toggle)
);

// Board LED numbering follows leds[0]..leds[5]:
// 1 heartbeat, 2/3 synchronized buttons, 4 frame sequence, 5 UART busy,
// 6 constant fabric-alive marker. The target wrapper handles active-low pins.
assign leds = {
    1'b1,
    uart_busy,
    frame_toggle,
    button_sync[1],
    button_sync[0],
    heartbeat[23]
};

endmodule
