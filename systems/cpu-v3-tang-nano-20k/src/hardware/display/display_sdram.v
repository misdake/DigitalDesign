// Single-upstream word/line adapter for the fitted Controller HS port. The
// memory arbiter owns all master selection and priority; this adapter only
// sequences one accepted CPU/arbiter transaction (legacy 16-bit word or a
// 1..4-line burst of one to sixteen 64-bit beats) against the SDRAM controller.
//
// `cpu_line_count_minus_1[1:0]` selects 1..4 consecutive 32-byte lines: the
// request transfers 4/8/12/16 ordered 64-bit beats and `last` marks the final
// beat. The request address must stay inside one 1 KiB SDRAM row; splitting a
// request that would cross a row is the requester's responsibility.
//
// Length zero keeps the legacy four-beat adapter staging because CPU caches do
// not expose per-beat backpressure. Longer GPU writes remain in their source
// cache entry and advance only when the related-clock width converter consumes
// a low/high pair. The gearbox holds one 64-bit pair, not a transaction buffer.
module SharedSdramPort (
    input wire clk, input wire reset,
    input wire cpu_request_valid, input wire cpu_write, input wire cpu_line,
    input wire [21:0] cpu_address, input wire [1:0] cpu_line_count_minus_1,
    input wire [63:0] cpu_write_data,
    input wire cpu_response_ready,
    input wire [63:0] controller_read_data, input wire controller_read_valid,
    input wire controller_init_done, input wire controller_command_ack,
    input wire controller_write_data_ready,
    output wire cpu_request_ready, output wire cpu_write_data_ready,
    output reg cpu_response_valid = 0,
    output reg [63:0] cpu_read_data = 0, output reg cpu_response_last = 0,
    output reg cpu_error = 0,
    output reg controller_command_valid = 0,
    output reg [2:0] controller_command = 3'b111,
    output reg controller_precharge = 0,
    output reg [20:0] controller_address = 0,
    output reg [3:0] controller_write_mask = 0,
    output wire [63:0] controller_write_data,
    output wire controller_write_data_valid,
    output reg [7:0] controller_burst_length = 0
);
localparam CMD_REFRESH=3'b001, CMD_ACTIVE=3'b011, CMD_WRITE=3'b100, CMD_READ=3'b101;
localparam ST_WAIT=0, ST_IDLE=1, ST_ACTIVE_REQ=2, ST_ACTIVE_WAIT=3,
           ST_OP_REQ=4, ST_OP_WAIT=5, ST_CPU_RESPONSE=6,
           ST_RECOVERY=7, ST_REFRESH_REQ=8, ST_REFRESH_WAIT=9, ST_ERROR=10,
           ST_WRITE_CAPTURE=11, ST_GEARBOX_PRELOAD=14;
localparam [19:0] TIMEOUT=20'hfffff;
reg [3:0] state = ST_WAIT;
reg pending_write = 0, pending_line = 0;
reg [21:0] pending_address = 0;
reg [1:0] pending_line_count = 0;
// Lane-positioned half-word held across the four-beat word-write stage.
reg [63:0] word_write_data = 0;
// Only the legacy one-line path is staged here. Long writes live in their
// source cache entry and advance on cpu_write_data_ready.
(* syn_ramstyle = "registers" *) reg [63:0] line_write_buffer [0:3];
reg [4:0] line_fed = 0;
reg [4:0] line_total = 0;
reg write_stream_delay = 0;
reg [3:0] beat = 0;
reg read_ack_seen = 0;
reg [9:0] refresh_count = 0;
reg [19:0] timeout_count = 0;
reg [2:0] recovery_count = 0;

wire refresh_due = refresh_count >= 10'd600;
assign cpu_request_ready = state == ST_IDLE && controller_init_done;
wire preloading_line_write = pending_write && pending_line &&
    state == ST_GEARBOX_PRELOAD && line_fed < line_total;
wire streaming_line_write = pending_write && pending_line &&
    state == ST_OP_WAIT && !write_stream_delay && line_fed < line_total;
wire feeding_line_write = preloading_line_write || streaming_line_write;
wire feeding_long_write = feeding_line_write && pending_line_count != 0;
assign cpu_write_data_ready = feeding_long_write && controller_write_data_ready;
assign controller_write_data_valid = pending_write &&
    ((pending_line && feeding_line_write) ||
     (!pending_line && state == ST_GEARBOX_PRELOAD));
// Legacy one-line writes stage through the local buffer. Long writes and word
// writes present the source/held value directly.
assign controller_write_data =
    pending_line_count != 0 ? cpu_write_data :
    (pending_line ? line_write_buffer[beat[1:0]] : word_write_data);

always @(posedge clk) begin
    controller_command_valid <= 0;
    if (controller_init_done && state != ST_REFRESH_WAIT && !refresh_due)
        refresh_count <= refresh_count + 1'b1;
    if (reset || !controller_init_done) begin
        state <= ST_WAIT; cpu_response_valid <= 0; cpu_response_last <= 0; cpu_error <= 0;
        refresh_count <= 0;
        pending_line_count <= 0; line_fed <= 0; line_total <= 0;
        write_stream_delay <= 0;
    end else begin
        case (state)
        ST_WAIT: begin refresh_count <= 0; state <= ST_IDLE; end
        ST_IDLE: begin
            // Ready is asserted throughout ST_IDLE, so an offered request must
            // win this collision. The overdue refresh runs after this bounded
            // transaction instead of silently dropping an accepted owner.
            if (cpu_request_valid) begin
                pending_write <= cpu_write;
                pending_line <= cpu_line;
                pending_address <= cpu_address;
                pending_line_count <= cpu_line ? cpu_line_count_minus_1 : 2'b00;
                line_total <= {cpu_line_count_minus_1, 2'b00} + 5'd4;
                if (cpu_write && cpu_line) begin
                    line_write_buffer[0] <= cpu_write_data;
                    if (cpu_line_count_minus_1 == 2'b00) begin
                        // Legacy four-beat staging: capture beats 1..3 before
                        // ACTIVE because this source has no beat-ready input.
                        beat <= 4'd1;
                        state <= ST_WRITE_CAPTURE;
                    end else begin
                        // Preload the related-clock holding register. Every
                        // later beat stays in the source cache.
                        line_fed <= 0;
                        state <= ST_GEARBOX_PRELOAD;
                    end
                end else if (cpu_write) begin
                    // A word write has no 64-bit CPU stream to capture; hold
                    // this lane-positioned value through gearbox preload so the
                    // width converter can present it for the one-beat WRITE.
                    word_write_data <= cpu_address[0] ?
                        {32'b0,cpu_write_data[15:0],16'b0} :
                        {48'b0,cpu_write_data[15:0]};
                    beat <= 0;
                    state <= ST_GEARBOX_PRELOAD;
                end else begin
                    state <= ST_ACTIVE_REQ;
                end
            end else if (refresh_due) state <= ST_REFRESH_REQ;
        end
        // Legacy one-line capture: three more 64-bit beats into the ring.
        ST_WRITE_CAPTURE: begin
            line_write_buffer[beat[1:0]] <= cpu_write_data;
            if (beat == 4'd3) begin
                beat <= 0;
                line_fed <= 0;
                state <= ST_GEARBOX_PRELOAD;
            end
            else beat <= beat + 1'b1;
        end
        ST_GEARBOX_PRELOAD: if (controller_write_data_ready) begin
            line_fed <= line_fed + 5'd1;
            if (pending_line_count == 0)
                beat <= beat + 1'b1;
            if (line_fed == 0)
                state <= ST_ACTIVE_REQ;
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
                cpu_error <= 1;
                state <= ST_ERROR;
            end else timeout_count <= timeout_count + 1'b1;
        end
        ST_OP_REQ: begin
            controller_command <= pending_write ? CMD_WRITE : CMD_READ;
            controller_precharge <= 1; controller_address <= pending_address[21:1];
            // Eight controller 32-bit beats per line, so 7/15/23/31 for
            // 1/2/3/4 lines.
            controller_burst_length <= pending_line ?
                ({pending_line_count, 3'b000} + 8'd7) : 8'd0;
            // DQM is a write byte mask; driving a stale or half-word mask
            // during a read can suppress the corresponding read byte lanes on
            // the physical SDRAM. Reads must keep all lanes enabled.
            controller_write_mask <= pending_write && !pending_line ?
                (pending_address[0] ? 4'b0011 : 4'b1100) : 4'b0000;
            controller_command_valid <= 1;
            // The 108-MHz wrapper phase-aligns WRITE one controller cycle
            // later. Skip the first ST_OP_WAIT rising edge; the following edge
            // consumes the preloaded high half and can capture the next pair.
            if (pending_write && pending_line)
                write_stream_delay <= 1;
            if (!(pending_write && pending_line)) beat <= 0;
            read_ack_seen <= 0;
            timeout_count <= 0; state <= ST_OP_WAIT;
        end
        ST_OP_WAIT: begin
            if (controller_command_ack) read_ack_seen <= 1;
            if (pending_write && pending_line && write_stream_delay)
                write_stream_delay <= 0;
            if (feeding_line_write && controller_write_data_ready) begin
                line_fed <= line_fed + 5'd1;
                if (pending_line_count == 0)
                    beat <= beat + 1'b1;
            end
            if (pending_write && controller_command_ack) begin
                cpu_read_data <= 0; cpu_response_last <= 1;
                cpu_response_valid <= 1; state <= ST_CPU_RESPONSE;
            end else if (!pending_write && pending_line) begin
                // Line read: 4/8/12/16 unstallable 64-bit beats, one per cycle;
                // the sink must accept every beat while streaming.
                cpu_response_valid <= controller_read_valid;
                if (controller_read_valid) begin
                    cpu_read_data <= controller_read_data;
                    cpu_response_last <= beat == line_total - 5'd1;
                    if (beat == line_total - 5'd1) begin
                        recovery_count <= 0; state <= ST_RECOVERY;
                    end
                    else beat <= beat + 1'b1;
                end else if (timeout_count == TIMEOUT) begin
                    cpu_error <= 1; cpu_response_valid <= 1; cpu_response_last <= 1;
                    state <= ST_CPU_RESPONSE;
                end else timeout_count <= timeout_count + 1'b1;
            end else if (!pending_write && controller_read_valid) begin
                cpu_read_data <= {48'b0, pending_address[0] ? controller_read_data[31:16] : controller_read_data[15:0]};
                if (read_ack_seen || controller_command_ack) begin
                    cpu_response_last <= 1; cpu_response_valid <= 1; state <= ST_CPU_RESPONSE;
                end
            end else if (timeout_count == TIMEOUT) begin
                cpu_error <= 1; cpu_response_valid <= 1; cpu_response_last <= 1;
                state <= ST_CPU_RESPONSE;
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
end
endmodule
