module DisplaySdramPort (
    input wire clk, input wire reset,
    input wire cpu_request_valid, input wire cpu_write, input wire cpu_read_line,
    input wire [21:0] cpu_address, input wire [15:0] cpu_write_data,
    input wire cpu_response_ready,
    input wire display_request_valid, input wire display_urgent,
    input wire [21:0] display_address,
    input wire [31:0] controller_read_data, input wire controller_read_valid,
    input wire controller_init_done, input wire controller_command_ack,
    output wire cpu_request_ready, output reg cpu_response_valid = 0,
    output reg [31:0] cpu_read_data = 0, output reg cpu_response_last = 0,
    output reg cpu_error = 0,
    output wire display_request_ready, output reg display_data_valid = 0,
    output reg [31:0] display_read_data = 0, output reg display_last = 0,
    output reg display_error = 0,
    output reg controller_command_valid = 0,
    output reg [2:0] controller_command = 3'b111,
    output reg controller_precharge = 0,
    output reg [20:0] controller_address = 0,
    output reg [3:0] controller_write_mask = 0,
    output reg [31:0] controller_write_data = 0,
    output reg [7:0] controller_burst_length = 0
);
localparam CMD_REFRESH=3'b001, CMD_ACTIVE=3'b011, CMD_WRITE=3'b100, CMD_READ=3'b101;
localparam ST_WAIT=0, ST_IDLE=1, ST_ACTIVE_REQ=2, ST_ACTIVE_WAIT=3,
           ST_OP_REQ=4, ST_OP_WAIT=5, ST_CPU_RESPONSE=6,
           ST_RECOVERY=7, ST_REFRESH_REQ=8, ST_REFRESH_WAIT=9, ST_ERROR=10;
localparam [19:0] TIMEOUT=20'hfffff;
reg [3:0] state = ST_WAIT;
reg owner_display = 0, pending_write = 0, pending_line = 0, prefer_display = 1;
reg [21:0] pending_address = 0;
reg [15:0] pending_write_data = 0;
reg [2:0] beat = 0;
reg read_ack_seen = 0;
reg [9:0] refresh_count = 0;
reg [19:0] timeout_count = 0;
reg [2:0] recovery_count = 0;

wire refresh_due = refresh_count >= 10'd600;
wire refresh_grant, display_grant, cpu_grant, next_prefer_display;
__DISPLAY_GRANT__ u_grant(
    .refresh_due(refresh_due), .display_valid(display_request_valid),
    .display_urgent(display_urgent), .cpu_valid(cpu_request_valid),
    .prefer_display(prefer_display), .refresh_grant(refresh_grant),
    .display_grant(display_grant), .cpu_grant(cpu_grant),
    .next_prefer_display(next_prefer_display)
);
assign cpu_request_ready = state == ST_IDLE && controller_init_done && cpu_grant;
assign display_request_ready = state == ST_IDLE && controller_init_done && display_grant;

always @(posedge clk) begin
    controller_command_valid <= 0;
    display_data_valid <= 0;
    display_last <= 0;
    if (controller_init_done && state != ST_REFRESH_WAIT && !refresh_due)
        refresh_count <= refresh_count + 1'b1;
    if (reset || !controller_init_done) begin
        state <= ST_WAIT; cpu_response_valid <= 0; cpu_response_last <= 0; cpu_error <= 0;
        display_error <= 0; refresh_count <= 0; prefer_display <= 1;
    end else case (state)
        ST_WAIT: begin refresh_count <= 0; state <= ST_IDLE; end
        ST_IDLE: begin
            if (refresh_grant) state <= ST_REFRESH_REQ;
            else if (display_grant || cpu_grant) begin
                owner_display <= display_grant;
                pending_write <= cpu_grant && cpu_write;
                pending_line <= cpu_grant && !cpu_write && cpu_read_line;
                pending_address <= display_grant ? display_address : cpu_address;
                pending_write_data <= cpu_write_data;
                prefer_display <= next_prefer_display;
                state <= ST_ACTIVE_REQ;
            end
        end
        ST_ACTIVE_REQ: begin
            controller_command <= CMD_ACTIVE; controller_precharge <= 0;
            controller_address <= pending_address[21:1];
            controller_command_valid <= 1; timeout_count <= 0;
            state <= ST_ACTIVE_WAIT;
        end
        ST_ACTIVE_WAIT: begin
            if (controller_command_ack) state <= ST_OP_REQ;
            else if (timeout_count == TIMEOUT) begin
                if (owner_display) display_error <= 1; else cpu_error <= 1;
                state <= ST_ERROR;
            end else timeout_count <= timeout_count + 1'b1;
        end
        ST_OP_REQ: begin
            controller_command <= pending_write ? CMD_WRITE : CMD_READ;
            controller_precharge <= 1; controller_address <= pending_address[21:1];
            controller_burst_length <= !pending_write && (owner_display || pending_line) ? 8'd7 : 8'd0;
            // DQM is a write byte mask; driving a stale or half-word mask
            // during a read can suppress the corresponding read byte lanes on
            // the physical SDRAM. Reads must keep all lanes enabled.
            controller_write_mask <= pending_write ?
                (pending_address[0] ? 4'b0011 : 4'b1100) : 4'b0000;
            controller_write_data <= pending_address[0] ?
                {pending_write_data,16'b0} : {16'b0,pending_write_data};
            controller_command_valid <= 1; beat <= 0; read_ack_seen <= 0;
            timeout_count <= 0; state <= ST_OP_WAIT;
        end
        ST_OP_WAIT: begin
            if (controller_command_ack) read_ack_seen <= 1;
            if (pending_write && controller_command_ack) begin
                cpu_read_data <= 0; cpu_response_last <= 1;
                cpu_response_valid <= 1; state <= ST_CPU_RESPONSE;
            end else if (!pending_write && owner_display && controller_read_valid) begin
                display_read_data <= controller_read_data;
                display_data_valid <= 1; display_last <= beat == 7;
                if (beat == 7) begin recovery_count <= 0; state <= ST_RECOVERY; end
                else beat <= beat + 1'b1;
            end else if (!pending_write && pending_line) begin
                // CPU line read: eight unstallable 32-bit beats, one per
                // cycle; the sink must accept every beat while streaming.
                cpu_response_valid <= controller_read_valid;
                if (controller_read_valid) begin
                    cpu_read_data <= controller_read_data;
                    cpu_response_last <= beat == 7;
                    if (beat == 7) begin recovery_count <= 0; state <= ST_RECOVERY; end
                    else beat <= beat + 1'b1;
                end else if (timeout_count == TIMEOUT) begin
                    cpu_error <= 1; cpu_response_valid <= 1; cpu_response_last <= 1;
                    state <= ST_CPU_RESPONSE;
                end else timeout_count <= timeout_count + 1'b1;
            end else if (!pending_write && controller_read_valid) begin
                cpu_read_data <= {16'b0, pending_address[0] ? controller_read_data[31:16] : controller_read_data[15:0]};
                if (read_ack_seen || controller_command_ack) begin
                    cpu_response_last <= 1; cpu_response_valid <= 1; state <= ST_CPU_RESPONSE;
                end
            end else if (timeout_count == TIMEOUT) begin
                if (owner_display) display_error <= 1; else begin cpu_error <= 1; cpu_response_valid <= 1; cpu_response_last <= 1; end
                state <= owner_display ? ST_ERROR : ST_CPU_RESPONSE;
            end else timeout_count <= timeout_count + 1'b1;
        end
        ST_CPU_RESPONSE: if (cpu_response_ready) begin
            cpu_response_valid <= 0; cpu_response_last <= 0; cpu_error <= 0;
            recovery_count <= 0; state <= ST_RECOVERY;
        end
        // The final line beat stays visible for the first recovery cycle;
        // the sink consumes it there because line beats are never stalled.
        ST_RECOVERY: begin
            cpu_response_valid <= 0;
            if (recovery_count == 3) state <= refresh_due ? ST_REFRESH_REQ : ST_IDLE;
            else recovery_count <= recovery_count + 1'b1;
        end
        ST_REFRESH_REQ: begin
            controller_command <= CMD_REFRESH; controller_precharge <= 0;
            controller_command_valid <= 1; timeout_count <= 0; state <= ST_REFRESH_WAIT;
        end
        ST_REFRESH_WAIT: if (controller_command_ack) begin refresh_count <= 0; state <= ST_IDLE; end
                         else if (timeout_count == TIMEOUT) state <= ST_ERROR;
                         else timeout_count <= timeout_count + 1'b1;
        default: state <= ST_ERROR;
    endcase
end
endmodule
