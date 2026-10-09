// Asynchronous SSRAM read, registered two-head consumer. DEPTH is logical 2..32.
module gpu_v2_texture_work #(
    parameter DEPTH = 16,
    parameter A = $clog2(DEPTH),
    parameter Q = $clog2(DEPTH + 1),
    parameter PHYSICAL_ROWS = (DEPTH <= 16 ? 16 : 32)
) (
    input wire clk, reset, ce,
    input wire in_valid,
    input wire [91:0] in_member,
    output wire in_ready,
    input wire out_ready,
    output wire out_valid,
    output wire [91:0] out_member,
    output wire [1:0] out_tap,
    output wire read_fire, write_fire, capture_fire, ack,
    output wire [A-1:0] read_row, write_row,
    output wire [Q-1:0] materialized,
    output wire [1:0] loaded,
    output reg fault
);
    reg [93:0] rows [0:PHYSICAL_ROWS-1];
    reg [91:0] heads [0:1];
    reg [1:0] cursors [0:1];
    reg [1:0] head_valid;
    reg consume_head, fill_head;
    reg [A-1:0] reclaim_ptr, fetch_ptr, write_ptr;
    reg [Q-1:0] count;
    reg [1:0] head_count;

    wire [93:0] read_data = rows[fetch_ptr];
    wire [3:0] emit = out_member[39:36];
    wire [3:0] later = emit & (4'b1110 << out_tap);
    wire take = ce & out_ready & out_valid & ~fault;
    wire put = ce & in_valid & in_ready;
    wire fetch = ce & ~reset & ~fault & (head_count < 2) & (count > head_count);
    wire last = (later == 0);
    wire bad = (put & (in_member[39:36] == 0)) |
        (fetch & ((read_data[93:92] != 0) | (read_data[39:36] == 0) |
                  head_valid[fill_head] | (put & (fetch_ptr == write_ptr)))) |
        (take & ~emit[out_tap]);

    function [1:0] first_tap;
        input [3:0] mask;
        begin
            if (mask[0]) first_tap = 0;
            else if (mask[1]) first_tap = 1;
            else if (mask[2]) first_tap = 2;
            else first_tap = 3;
        end
    endfunction

    assign in_ready = ~reset & ~fault & (count < DEPTH);
    assign out_valid = ~reset & ~fault & head_valid[consume_head];
    assign out_member = heads[consume_head];
    assign out_tap = cursors[consume_head];
    assign read_fire = fetch;
    assign write_fire = put;
    assign capture_fire = take;
    assign ack = take & last;
    assign read_row = fetch_ptr;
    assign write_row = write_ptr;
    assign materialized = count;
    assign loaded = head_count;

    always @(posedge clk) begin
        if (reset) begin
            head_valid <= 0;
            consume_head <= 0; fill_head <= 0;
            reclaim_ptr <= 0; fetch_ptr <= 0; write_ptr <= 0;
            count <= 0; head_count <= 0; fault <= 0;
        end else if (ce & ~fault) begin
            if (bad) begin
                fault <= 1;
            end else begin
                case ({put, ack})
                    2'b10: count <= count + 1'b1;
                    2'b01: count <= count - 1'b1;
                    default: count <= count;
                endcase
                case ({fetch, ack})
                    2'b10: head_count <= head_count + 1'b1;
                    2'b01: head_count <= head_count - 1'b1;
                    default: head_count <= head_count;
                endcase
                if (put) begin
                    rows[write_ptr] <= {2'b00, in_member};
                    if (write_ptr == DEPTH-1) write_ptr <= 0;
                    else write_ptr <= write_ptr + 1'b1;
                end
                if (fetch) begin
                    heads[fill_head] <= read_data[91:0];
                    cursors[fill_head] <= first_tap(read_data[39:36]);
                    head_valid[fill_head] <= 1;
                    fill_head <= ~fill_head;
                    if (fetch_ptr == DEPTH-1) fetch_ptr <= 0;
                    else fetch_ptr <= fetch_ptr + 1'b1;
                end
                if (take) begin
                    if (last) begin
                        head_valid[consume_head] <= 0;
                        consume_head <= ~consume_head;
                        if (reclaim_ptr == DEPTH-1) reclaim_ptr <= 0;
                        else reclaim_ptr <= reclaim_ptr + 1'b1;
                    end else begin
                        cursors[consume_head] <= first_tap(later);
                    end
                end
            end
        end
    end
endmodule
