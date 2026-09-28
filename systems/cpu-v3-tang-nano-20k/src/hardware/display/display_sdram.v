// Adapt one arbiter-owned request to the native 32-bit SDRAM transaction.
// The controller owns rows, refresh and timing; the board bridge owns 54/108
// MHz transport. This module owns only 16-bit scalar and 64-bit line packing.
module SharedSdramPort (
    input wire clk,
    input wire reset,
    input wire cpu_request_valid,
    input wire cpu_write,
    input wire cpu_line,
    input wire [21:0] cpu_address,
    input wire [1:0] cpu_line_count_minus_1,
    input wire [63:0] cpu_write_data,
    input wire cpu_response_ready,
    input wire [63:0] controller_read_data,
    input wire controller_read_valid,
    input wire controller_init_done,
    input wire controller_request_ready,
    input wire controller_done,
    input wire controller_write_data_ready,
    output wire cpu_request_ready,
    output wire cpu_write_data_ready,
    output reg cpu_response_valid = 0,
    output reg [63:0] cpu_read_data = 0,
    output reg cpu_response_last = 0,
    output reg cpu_error = 0,
    output wire controller_request_valid,
    output wire controller_write,
    output wire [20:0] controller_address,
    output wire [3:0] controller_write_mask,
    output wire [63:0] controller_write_data,
    output wire controller_write_data_valid,
    output wire [5:0] controller_words
);
    localparam ST_IDLE = 0, ST_BUSY = 1, ST_READ_LAST = 2,
               ST_WAIT_DONE = 3, ST_RESPONSE = 4;
    localparam [19:0] TIMEOUT = 20'hfffff;
    reg [2:0] state = ST_IDLE;
    reg pending_write = 0;
    reg pending_line = 0;
    reg pending_lane = 0;
    reg [4:0] line_total = 0;
    reg [4:0] line_fed = 0;
    reg [4:0] read_beats = 0;
    reg [63:0] scalar_write_data = 0;
    reg [31:0] scalar_read_data = 0;
    reg scalar_read_seen = 0;
    reg done_seen = 0;
    reg [19:0] timeout_count = 0;

    wire length_ok = !cpu_line || cpu_line_count_minus_1 != 2'b10;
    wire alignment_ok = !cpu_line ||
        (cpu_line_count_minus_1 == 2'b00 && cpu_address[3:0] == 0) ||
        (cpu_line_count_minus_1 == 2'b01 && cpu_address[4:0] == 0) ||
        (cpu_line_count_minus_1 == 2'b11 && cpu_address[5:0] == 0);
    wire legal_request = length_ok && alignment_ok;
    wire active_write = state == ST_BUSY && pending_write &&
        line_fed < (pending_line ? line_total : 5'd1);

    assign cpu_request_ready = state == ST_IDLE && controller_init_done &&
        (!cpu_request_valid || !legal_request || controller_request_ready);
    assign controller_request_valid = state == ST_IDLE && cpu_request_valid &&
        controller_init_done && legal_request;
    assign controller_write = cpu_write;
    // The core's BANK_BIT=5 mapping is the only address permutation. This
    // conversion merely drops the 16-bit halfword lane bit.
    assign controller_address = cpu_address[21:1];
    assign controller_words = !cpu_line ? 6'd1 :
        cpu_line_count_minus_1 == 2'b00 ? 6'd8 :
        cpu_line_count_minus_1 == 2'b01 ? 6'd16 : 6'd32;
    assign controller_write_mask = cpu_write && !cpu_line ?
        (cpu_address[0] ? 4'b0011 : 4'b1100) : 4'b0000;
    assign controller_write_data = pending_line ? cpu_write_data : scalar_write_data;
    assign controller_write_data_valid = active_write;
    assign cpu_write_data_ready = active_write && pending_line && controller_write_data_ready;

    always @(posedge clk) begin
        if (reset || !controller_init_done) begin
            state <= ST_IDLE;
            cpu_response_valid <= 0;
            cpu_response_last <= 0;
            cpu_error <= 0;
            line_fed <= 0;
            scalar_read_seen <= 0;
            done_seen <= 0;
            timeout_count <= 0;
        end else begin
            case (state)
                ST_IDLE: if (cpu_request_valid && cpu_request_ready) begin
                    pending_write <= cpu_write;
                    pending_line <= cpu_line;
                    pending_lane <= cpu_address[0];
                    line_total <= {cpu_line_count_minus_1, 2'b00} + 5'd4;
                    line_fed <= 0;
                    read_beats <= 0;
                    scalar_read_seen <= 0;
                    done_seen <= 0;
                    timeout_count <= 0;
                    cpu_response_valid <= 0;
                    cpu_response_last <= 0;
                    cpu_error <= !legal_request;
                    if (!legal_request) begin
                        cpu_read_data <= 0;
                        cpu_response_valid <= 1;
                        cpu_response_last <= 1;
                        state <= ST_RESPONSE;
                    end else begin
                        scalar_write_data <= cpu_address[0] ?
                            {32'b0, cpu_write_data[15:0], 16'b0} :
                            {48'b0, cpu_write_data[15:0]};
                        state <= ST_BUSY;
                    end
                end

                ST_BUSY: begin
                    timeout_count <= timeout_count + 1'b1;
                    if (active_write && controller_write_data_ready)
                        line_fed <= line_fed + 1'b1;
                    if (controller_done)
                        done_seen <= 1;
                    if (!pending_write && controller_read_valid) begin
                        if (pending_line) begin
                            cpu_read_data <= controller_read_data;
                            cpu_response_valid <= 1;
                            cpu_response_last <= read_beats == line_total - 1'b1;
                            read_beats <= read_beats + 1'b1;
                            if (read_beats == line_total - 1'b1)
                                state <= ST_READ_LAST;
                        end else begin
                            scalar_read_data <= controller_read_data[31:0];
                            scalar_read_seen <= 1;
                        end
                    end else if (pending_line && !pending_write) begin
                        cpu_response_valid <= 0;
                    end
                    if (controller_done && pending_write) begin
                        cpu_read_data <= 0;
                        cpu_response_valid <= 1;
                        cpu_response_last <= 1;
                        state <= ST_RESPONSE;
                    end else if (!pending_write && !pending_line && (done_seen || controller_done) &&
                                 (scalar_read_seen || controller_read_valid)) begin
                        cpu_read_data <= {48'b0, pending_lane ?
                            (controller_read_valid ? controller_read_data[31:16] : scalar_read_data[31:16]) :
                            (controller_read_valid ? controller_read_data[15:0] : scalar_read_data[15:0])};
                        cpu_response_valid <= 1;
                        cpu_response_last <= 1;
                        state <= ST_RESPONSE;
                    end else if (timeout_count == TIMEOUT) begin
                        cpu_error <= 1;
                        cpu_response_valid <= 1;
                        cpu_response_last <= 1;
                        state <= ST_RESPONSE;
                    end
                end

                // The final 64-bit beat remains stable until the consumer
                // accepts it, even if completion is still in flight.
                ST_READ_LAST: if (cpu_response_ready) begin
                    cpu_response_valid <= 0;
                    cpu_response_last <= 0;
                    timeout_count <= 0;
                    if (done_seen || controller_done)
                        state <= ST_IDLE;
                    else
                        state <= ST_WAIT_DONE;
                end
                ST_WAIT_DONE: if (controller_done) begin
                    state <= ST_IDLE;
                end else if (timeout_count == TIMEOUT) begin
                    cpu_error <= 1;
                    cpu_response_valid <= 1;
                    cpu_response_last <= 1;
                    state <= ST_RESPONSE;
                end else timeout_count <= timeout_count + 1'b1;

                ST_RESPONSE: if (cpu_response_ready) begin
                    cpu_response_valid <= 0;
                    cpu_response_last <= 0;
                    cpu_error <= 0;
                    state <= ST_IDLE;
                end
                default: state <= ST_IDLE;
            endcase
        end
    end
endmodule
