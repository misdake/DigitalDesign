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

localparam [31:0] STARTUP_CYCLES = 32'd1_000_000;
localparam [31:0] GAP_CYCLES = 32'd5_000_000;

reg [1:0] button_meta = 0;
reg [1:0] button_sync = 0;
reg [31:0] heartbeat = 0;
always @(posedge clk) begin
    button_meta <= buttons;
    button_sync <= button_meta;
    heartbeat <= heartbeat + 1'b1;
end

reg [31:0] gap_counter = 0;
reg first_frame = 1;
reg frame_pending = 0;
reg frame_active = 0;
reg [2:0] frame_byte_index = 0;
reg [1:0] frame_status = 0;
reg [7:0] frame_sequence = 0;

reg [9:0] uart_frame = 10'h3ff;
reg [3:0] uart_bit = 0;
reg [7:0] uart_divider = 0;
reg uart_busy = 0;

function [7:0] report_byte;
    input [2:0] index;
    input [1:0] status;
    begin
        case (index)
            0: report_byte = 8'h44; // D
            1: report_byte = 8'h44; // D
            2: report_byte = 8'h48; // H
            3: report_byte = 8'h54; // T
            4: report_byte = 8'h01; // protocol version
            5: report_byte = 8'h0a; // Tang Nano 20K board health
            6: report_byte = {6'b0, status}; // non-zero: a reset button is high
            default: report_byte = 8'h17 ^ {6'b0, status};
        endcase
    end
endfunction

always @(posedge clk) begin
    if (!frame_pending && !frame_active && !uart_busy) begin
        if (gap_counter == (first_frame ? STARTUP_CYCLES : GAP_CYCLES)) begin
            gap_counter <= 0;
            first_frame <= 0;
            frame_status <= button_sync;
            frame_pending <= 1;
        end else begin
            gap_counter <= gap_counter + 1'b1;
        end
    end

    if (!uart_busy) begin
        if (frame_pending) begin
            frame_pending <= 0;
            frame_active <= 1;
            frame_byte_index <= 0;
        end else if (frame_active) begin
            uart_frame <= {1'b1, report_byte(frame_byte_index, frame_status), 1'b0};
            uart_bit <= 0;
            uart_divider <= 0;
            uart_busy <= 1;
            if (frame_byte_index == 7) begin
                frame_active <= 0;
                frame_sequence <= frame_sequence + 1'b1;
            end else begin
                frame_byte_index <= frame_byte_index + 1'b1;
            end
        end
    end else if (uart_divider == 8'd233) begin
        uart_divider <= 0;
        if (uart_bit == 9)
            uart_busy <= 0;
        else
            uart_bit <= uart_bit + 1'b1;
    end else begin
        uart_divider <= uart_divider + 1'b1;
    end
end

assign uart_tx = uart_busy ? uart_frame[uart_bit] : 1'b1;

// Board LED numbering follows leds[0]..leds[5]:
// 1 heartbeat, 2/3 synchronized buttons, 4 frame sequence, 5 UART busy,
// 6 constant fabric-alive marker. The target wrapper handles active-low pins.
assign leds = {
    1'b1,
    uart_busy,
    frame_sequence[0],
    button_sync[1],
    button_sync[0],
    heartbeat[23]
};

endmodule
