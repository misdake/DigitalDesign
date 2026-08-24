// Board characterization harness for the system-control device: no CPU and no
// MMIO bridge. An FSM drives the device register interface directly — one LED
// pattern write on channel 2, then the DDHT status frame for test ID 0x08
// repeated on channel 3 with gaps between frames. The LED pins mirror probe
// internals ({busy, 2'b01, state}) instead of the channel-2 register so a
// silent board still shows where the FSM sits.
module SysctlUartProbe (
    input wire clk,
    input wire [1:0] buttons,
    output wire [5:0] leds,
    output wire uart_tx
);

localparam [2:0] ST_STARTUP = 0;
localparam [2:0] ST_LED = 1;
localparam [2:0] ST_POLL = 2;
localparam [2:0] ST_WRITE = 3;
localparam [2:0] ST_GAP = 4;

// Short delays keep the simulation quick; on the board they simply set the
// frame cadence (about 37 ms startup and 185 ms between frames).
localparam [31:0] STARTUP_CYCLES = 32'd1_000_000;
localparam [31:0] GAP_CYCLES = 32'd5_000_000;

reg [2:0] state = ST_STARTUP;
reg [31:0] delay = 0;
reg [2:0] byte_index = 0;

wire device_read_enable = 1'b1;
reg device_write_enable = 0;
reg [3:0] device_channel = 0;
reg [15:0] device_write_data = 0;
wire [15:0] device_read_data;

function [7:0] frame_byte;
    input [2:0] index;
    begin
        case (index)
            0: frame_byte = 8'h44; // D
            1: frame_byte = 8'h44; // D
            2: frame_byte = 8'h48; // H
            3: frame_byte = 8'h54; // T
            4: frame_byte = 8'h01; // protocol version
            5: frame_byte = 8'h08; // test ID 0x08
            6: frame_byte = 8'h00; // status: success
            default: frame_byte = 8'h15; // XOR checksum of bytes 0..6
        endcase
    end
endfunction

always @(posedge clk) begin
    device_write_enable <= 0;
    case (state)
        ST_STARTUP: begin
            if (delay == STARTUP_CYCLES) begin
                delay <= 0;
                device_channel <= 2;
                device_write_data <= 16'h0015;
                device_write_enable <= 1;
                state <= ST_LED;
            end else begin
                delay <= delay + 1'b1;
            end
        end
        ST_LED: begin
            byte_index <= 0;
            state <= ST_POLL;
        end
        ST_POLL: begin
            device_channel <= 3;
            // device_read_enable is tied high; the busy readback on channel 3
            // is valid one cycle after device_channel settles.
            if (device_channel == 3 && device_read_data[0] == 0) begin
                device_write_data <= {8'b0, frame_byte(byte_index)};
                device_write_enable <= 1;
                state <= ST_WRITE;
            end
        end
        ST_WRITE: begin
            if (byte_index == 7) begin
                state <= ST_GAP;
            end else begin
                byte_index <= byte_index + 1'b1;
                state <= ST_POLL;
            end
        end
        ST_GAP: begin
            if (delay == GAP_CYCLES) begin
                delay <= 0;
                byte_index <= 0;
                state <= ST_POLL;
            end else begin
                delay <= delay + 1'b1;
            end
        end
        default: state <= ST_STARTUP;
    endcase
end

// Debug variant: LEDs mirror the probe state directly (not through channel 2)
// so the stuck point is visible on the board: led[5]=busy readback,
// led[4:3]=frame progress, led[2:0]=state.
assign leds = {device_read_data[0], 2'b01, state};

SystemControlDevice_CLOCKS_PER_BIT234 u_sysctl (
    .clk(clk),
    .reset(1'b0),
    .device_index(4'd0),
    .device_channel(device_channel),
    .device_read_enable(device_read_enable),
    .device_write_enable(device_write_enable),
    .device_write_data(device_write_data),
    .device_read_data(device_read_data),
    .icache_invalidate(),
    .dcache_invalidate(),
    .leds(),
    .uart_tx(uart_tx)
);

endmodule
