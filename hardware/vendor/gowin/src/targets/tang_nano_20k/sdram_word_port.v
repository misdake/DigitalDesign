module TangNano20KSdramWordPort (
    input wire clk,
    input wire reset,
    input wire request_valid,
    input wire write,
    input wire [21:0] address,
    input wire [15:0] write_data,
    input wire response_ready,
    input wire [31:0] controller_read_data,
    input wire controller_read_valid,
    input wire controller_init_done,
    input wire controller_command_ack,
    output wire request_ready,
    output reg response_valid = 1'b0,
    output reg [15:0] read_data = 16'b0,
    output reg error = 1'b0,
    output reg controller_command_valid = 1'b0,
    output reg [2:0] controller_command = 3'b111,
    output reg controller_precharge = 1'b0,
    output reg [20:0] controller_address = 21'b0,
    output reg [3:0] controller_write_mask = 4'b0,
    output reg [31:0] controller_write_data = 32'b0,
    output reg [7:0] controller_burst_length = 8'b0
);
localparam [2:0] CMD_REFRESH = 3'b001;
localparam [2:0] CMD_ACTIVE = 3'b011;
localparam [2:0] CMD_WRITE = 3'b100;
localparam [2:0] CMD_READ = 3'b101;
localparam [9:0] REFRESH_INTERVAL = 10'd600;
localparam [19:0] TIMEOUT = 20'hfffff;

localparam [3:0] ST_WAIT_INIT = 0;
localparam [3:0] ST_IDLE = 1;
localparam [3:0] ST_ACTIVE_REQUEST = 2;
localparam [3:0] ST_ACTIVE_WAIT = 3;
localparam [3:0] ST_OPERATION_REQUEST = 4;
localparam [3:0] ST_OPERATION_WAIT = 5;
localparam [3:0] ST_RESPONSE = 6;
localparam [3:0] ST_REFRESH_REQUEST = 7;
localparam [3:0] ST_REFRESH_WAIT = 8;
localparam [3:0] ST_ERROR = 9;

reg [3:0] state = ST_WAIT_INIT;
reg pending_write = 1'b0;
reg [21:0] pending_address = 22'b0;
reg [15:0] pending_write_data = 16'b0;
reg read_ack_seen = 1'b0;
reg read_data_seen = 1'b0;
reg [9:0] refresh_count = 10'b0;
reg [19:0] timeout_count = 20'b0;

wire refresh_due = refresh_count >= REFRESH_INTERVAL;
assign request_ready = state == ST_IDLE && controller_init_done && !refresh_due;

always @(posedge clk) begin
    controller_command_valid <= 1'b0;
    if (controller_init_done && state != ST_REFRESH_WAIT && !refresh_due)
        refresh_count <= refresh_count + 1'b1;

    if (reset || !controller_init_done) begin
        state <= ST_WAIT_INIT;
        response_valid <= 1'b0;
        error <= 1'b0;
        refresh_count <= 0;
        timeout_count <= 0;
    end else begin
        case (state)
            ST_WAIT_INIT: begin
                refresh_count <= 0;
                state <= ST_IDLE;
            end

            ST_IDLE: begin
                if (refresh_due) begin
                    state <= ST_REFRESH_REQUEST;
                end else if (request_valid) begin
                    pending_write <= write;
                    pending_address <= address;
                    pending_write_data <= write_data;
                    state <= ST_ACTIVE_REQUEST;
                end
            end

            ST_ACTIVE_REQUEST: begin
                controller_command <= CMD_ACTIVE;
                controller_precharge <= 1'b0;
                controller_address <= pending_address[21:1];
                controller_command_valid <= 1'b1;
                timeout_count <= 0;
                state <= ST_ACTIVE_WAIT;
            end

            ST_ACTIVE_WAIT: begin
                if (controller_command_ack) begin
                    state <= ST_OPERATION_REQUEST;
                end else if (timeout_count == TIMEOUT) begin
                    error <= 1'b1;
                    state <= ST_ERROR;
                end else begin
                    timeout_count <= timeout_count + 1'b1;
                end
            end

            ST_OPERATION_REQUEST: begin
                controller_command <= pending_write ? CMD_WRITE : CMD_READ;
                controller_precharge <= 1'b1;
                controller_address <= pending_address[21:1];
                controller_burst_length <= 0;
                if (pending_address[0]) begin
                    controller_write_mask <= 4'b0011;
                    controller_write_data <= {pending_write_data, 16'b0};
                end else begin
                    controller_write_mask <= 4'b1100;
                    controller_write_data <= {16'b0, pending_write_data};
                end
                controller_command_valid <= 1'b1;
                read_ack_seen <= 1'b0;
                read_data_seen <= 1'b0;
                timeout_count <= 0;
                state <= ST_OPERATION_WAIT;
            end

            ST_OPERATION_WAIT: begin
                if (pending_write) begin
                    if (controller_command_ack) begin
                        read_data <= 0;
                        response_valid <= 1'b1;
                        state <= ST_RESPONSE;
                    end else if (timeout_count == TIMEOUT) begin
                        error <= 1'b1;
                        state <= ST_ERROR;
                    end else begin
                        timeout_count <= timeout_count + 1'b1;
                    end
                end else begin
                    if (controller_command_ack)
                        read_ack_seen <= 1'b1;
                    if (controller_read_valid) begin
                        read_data <= pending_address[0] ?
                            controller_read_data[31:16] : controller_read_data[15:0];
                        read_data_seen <= 1'b1;
                    end
                    if ((read_ack_seen || controller_command_ack) &&
                        (read_data_seen || controller_read_valid)) begin
                        response_valid <= 1'b1;
                        state <= ST_RESPONSE;
                    end else if (timeout_count == TIMEOUT) begin
                        error <= 1'b1;
                        state <= ST_ERROR;
                    end else begin
                        timeout_count <= timeout_count + 1'b1;
                    end
                end
            end

            ST_RESPONSE: begin
                if (response_ready) begin
                    response_valid <= 1'b0;
                    state <= refresh_due ? ST_REFRESH_REQUEST : ST_IDLE;
                end else if (refresh_due) begin
                    state <= ST_REFRESH_REQUEST;
                end
            end

            ST_REFRESH_REQUEST: begin
                controller_command <= CMD_REFRESH;
                controller_precharge <= 1'b0;
                controller_command_valid <= 1'b1;
                timeout_count <= 0;
                state <= ST_REFRESH_WAIT;
            end

            ST_REFRESH_WAIT: begin
                if (controller_command_ack) begin
                    refresh_count <= 0;
                    state <= response_valid ? ST_RESPONSE : ST_IDLE;
                end else if (timeout_count == TIMEOUT) begin
                    error <= 1'b1;
                    state <= ST_ERROR;
                end else begin
                    timeout_count <= timeout_count + 1'b1;
                end
            end

            default: begin
                response_valid <= 1'b0;
                error <= 1'b1;
                state <= ST_ERROR;
            end
        endcase
    end
end
endmodule
