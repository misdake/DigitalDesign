// Adapt one arbiter-owned request to the native 32-bit SDRAM transaction.
// The controller owns rows, refresh and timing; the board bridge owns 54/108
// MHz transport. This module owns only 16-bit scalar and 64-bit line packing.
module SharedSdramPort #(parameter EARLY_GRANT = 0, parameter CHAIN_GROUP_FOUR = 0) (
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
    input wire controller_stream_active,
    input wire controller_done,
    input wire controller_write_data_ready,
    output wire cpu_request_ready,
    output wire cpu_lookahead_window,
    output wire cpu_write_data_ready,
    output reg cpu_response_valid = 0,
    output reg [63:0] cpu_read_data = 0,
    output reg cpu_response_last = 0,
    output reg cpu_error = 0,
    output wire controller_request_valid,
    output wire controller_next_valid,
    output wire [20:0] controller_next_address,
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
    reg [6:0] line_total = 0;
    reg [6:0] line_fed = 0;
    reg [6:0] read_beats = 0;
    reg [63:0] scalar_write_data = 0;
    reg [31:0] scalar_read_data = 0;
    reg scalar_read_seen = 0;
    reg done_seen = 0;
    reg [19:0] timeout_count = 0;
    // One irrevocably granted descriptor may wait behind the active request.
    // Its line payload remains with the granted arbiter owner until promotion.
    reg queued_valid = 0;
    reg queued_write = 0;
    reg queued_line = 0;
    reg [21:0] queued_address = 0;
    reg [1:0] queued_count = 0;
    reg [63:0] queued_scalar_data = 0;
    reg queued_legal = 0;
    reg terminal_write_hold = 0;

    // The otherwise-illegal code 2 is an explicit, indivisible four-sector
    // group only in this opt-in configuration. Default production is unchanged.
    wire group_request = CHAIN_GROUP_FOUR && cpu_line && cpu_line_count_minus_1 == 2'b10;
    wire length_ok = !cpu_line || cpu_line_count_minus_1 != 2'b10 || group_request;
    wire alignment_ok = !cpu_line ||
        (cpu_line_count_minus_1 == 2'b00 && cpu_address[3:0] == 0) ||
        (cpu_line_count_minus_1 == 2'b01 && cpu_address[4:0] == 0) ||
        (cpu_line_count_minus_1 == 2'b11 && cpu_address[5:0] == 0) ||
        (group_request && cpu_address[7:0] == 0);
    wire legal_request = length_ok && alignment_ok;
    wire launch_write = queued_valid ? queued_write : cpu_write;
    wire launch_line = queued_valid ? queued_line : cpu_line;
    wire [21:0] launch_address = queued_valid ? queued_address : cpu_address;
    wire [1:0] launch_count = queued_valid ? queued_count : cpu_line_count_minus_1;
    wire [63:0] launch_scalar_data = queued_valid ? queued_scalar_data :
        cpu_write_data;
    wire launch_legal = queued_valid ? queued_legal : legal_request;
    wire launch_valid = queued_valid || cpu_request_valid;
    wire launch_ready = !launch_legal || controller_request_ready;
    wire terminal_launch_slot = queued_valid && !cpu_error && cpu_response_ready &&
        (state == ST_RESPONSE ||
         (state == ST_READ_LAST && (done_seen || controller_done)));
    wire active_write = state == ST_BUSY && pending_write &&
        line_fed < (pending_line ? line_total : 7'd1);
    wire [7:0] progress_plus_four = {1'b0, pending_write ? line_fed : read_beats} + 8'd4;
    wire launch_group = CHAIN_GROUP_FOUR && launch_line && launch_count == 2'b10;

    // Open the sole successor slot near the tail of a line transfer. A grant
    // made here is final even when a higher-priority request arrives later.
    assign cpu_lookahead_window = EARLY_GRANT && state == ST_BUSY && controller_stream_active && pending_line &&
        !queued_valid && progress_plus_four >= {1'b0, line_total};

    assign cpu_request_ready = controller_init_done && !queued_valid &&
        (state == ST_IDLE ? (!cpu_request_valid || launch_ready) : cpu_lookahead_window);
    assign controller_request_valid = (state == ST_IDLE || terminal_launch_slot) && launch_valid &&
        controller_init_done && launch_legal;
    assign controller_next_valid = queued_valid && queued_legal && state == ST_BUSY;
    assign controller_next_address = queued_address[21:1];
    assign controller_write = launch_write;
    // The core's BANK_BIT=5 mapping is the only address permutation. This
    // conversion merely drops the 16-bit halfword lane bit.
    assign controller_address = launch_address[21:1];
    assign controller_words = !launch_line ? 6'd1 :
        launch_count == 2'b00 ? 6'd8 :
        launch_count == 2'b01 ? 6'd16 : launch_group ? 6'd0 : 6'd32;
    assign controller_write_mask = launch_write && !launch_line ?
        (launch_address[0] ? 4'b0011 : 4'b1100) : 4'b0000;
    assign controller_write_data = pending_line ? cpu_write_data : scalar_write_data;
    assign controller_write_data_valid = active_write && !terminal_write_hold;
    assign cpu_write_data_ready = active_write && !terminal_write_hold &&
        pending_line && controller_write_data_ready;

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
            queued_valid <= 0;
            terminal_write_hold <= 0;
        end else begin
            terminal_write_hold <= 0;
            if ((EARLY_GRANT || CHAIN_GROUP_FOUR) && state == ST_READ_LAST && controller_done) done_seen <= 1;
            if (state != ST_IDLE && cpu_request_valid && cpu_request_ready) begin
                queued_valid <= 1;
                queued_write <= cpu_write;
                queued_line <= cpu_line;
                queued_address <= cpu_address;
                queued_count <= cpu_line_count_minus_1;
                queued_scalar_data <= cpu_write_data;
                queued_legal <= legal_request;
            end
            case (state)
                ST_IDLE: if (launch_valid && launch_ready) begin
                    queued_valid <= 0;
                    pending_write <= launch_write;
                    pending_line <= launch_line;
                    pending_lane <= launch_address[0];
                    line_total <= launch_group ? 7'd64 : {3'b0, launch_count, 2'b00} + 7'd4;
                    line_fed <= 0;
                    read_beats <= 0;
                    scalar_read_seen <= 0;
                    done_seen <= 0;
                    timeout_count <= 0;
                    cpu_response_valid <= 0;
                    cpu_response_last <= 0;
                    cpu_error <= !launch_legal;
                    if (!launch_legal) begin
                        cpu_read_data <= 0;
                        cpu_response_valid <= 1;
                        cpu_response_last <= 1;
                        state <= ST_RESPONSE;
                    end else begin
                        scalar_write_data <= launch_address[0] ?
                            {32'b0, launch_scalar_data[15:0], 16'b0} :
                            {48'b0, launch_scalar_data[15:0]};
                        state <= ST_BUSY;
                    end
                end

                ST_BUSY: begin
                    timeout_count <= timeout_count + 1'b1;
                    if (controller_write_data_valid && controller_write_data_ready)
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
            // The successor was already granted, so it can enter the native
            // bridge on the same edge that the previous final response retires.
            if (terminal_launch_slot && launch_ready) begin
                queued_valid <= 0;
                // A GPU clean changes its cache read address on this edge.
                // Hold the new write stream for its synchronous prime cycle.
                terminal_write_hold <= launch_write && launch_line;
                pending_write <= launch_write;
                pending_line <= launch_line;
                pending_lane <= launch_address[0];
                line_total <= launch_group ? 7'd64 : {3'b0, launch_count, 2'b00} + 7'd4;
                line_fed <= 0;
                read_beats <= 0;
                scalar_read_seen <= 0;
                done_seen <= 0;
                timeout_count <= 0;
                cpu_response_valid <= 0;
                cpu_response_last <= 0;
                cpu_error <= !launch_legal;
                if (!launch_legal) begin
                    cpu_read_data <= 0;
                    cpu_response_valid <= 1;
                    cpu_response_last <= 1;
                    state <= ST_RESPONSE;
                end else begin
                    scalar_write_data <= launch_address[0] ?
                        {32'b0, launch_scalar_data[15:0], 16'b0} :
                        {48'b0, launch_scalar_data[15:0]};
                    state <= ST_BUSY;
                end
            end
        end
    end
endmodule
