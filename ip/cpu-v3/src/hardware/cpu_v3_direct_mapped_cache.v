module CpuV3DirectMappedCache (
    input wire clk,
    input wire reset,
    input wire invalidate_all,
    input wire cpu_request_valid,
    input wire cpu_write,
    input wire [31:0] cpu_address,
    input wire [15:0] cpu_write_data,
    input wire cpu_response_ready,
    input wire memory_request_ready,
    input wire memory_response_valid,
    input wire [31:0] memory_read_data,
    input wire memory_error,
    output wire cpu_request_ready,
    output wire cpu_response_valid,
    output wire [15:0] cpu_read_data,
    output wire cpu_error,
    output wire memory_request_valid,
    output wire memory_write,
    output wire [21:0] memory_address,
    output wire [15:0] memory_write_data,
    output wire memory_response_ready
);

// A read miss issues one aligned line request and receives exactly eight
// ordered 32-bit beats; beat n carries word 2*n in its low half and word
// 2*n+1 in its high half. A write issues one word request and receives one
// completion response. An error beat terminates a line response early; no
// further beats follow it. The line commits to the data BSRAM and tag RAM
// only after a complete error-free line has landed in the private refill
// buffer, so an error or invalidate can never expose a partially installed
// line.

localparam [2:0] ST_IDLE = 0;
localparam [2:0] ST_CHECK = 1;
localparam [2:0] ST_WORD_REQUEST = 2;
localparam [2:0] ST_WORD_RESPONSE = 3;
localparam [2:0] ST_LINE_REQUEST = 4;
localparam [2:0] ST_LINE_RECEIVE = 5;
localparam [2:0] ST_LINE_DRAIN = 6;
localparam [2:0] ST_CPU_RESPONSE = 7;

reg [2:0] state = ST_IDLE;
reg pending_write = 0;
reg [31:0] pending_address = 0;
reg [15:0] pending_write_data = 0;
reg [2:0] refill_beat = 0;
reg [3:0] drain_word = 0;
// The refill buffer is plain flip-flops, not inferred RAM: it is the future
// CPU/DRAM clock-domain crossing structure and must stay a register array.
(* syn_ramstyle = "registers" *) reg [31:0] refill_buffer [0:7];
reg [15:0] response_data = 0;
reg response_error = 0;
reg [63:0] valid = __INITIAL_VALID__;

wire cpu_address_valid = cpu_address[31:22] == 0;

wire [5:0] pending_set = pending_address[9:4];
wire [11:0] pending_tag = pending_address[21:10];
wire [3:0] pending_word = pending_address[3:0];
wire [11:0] tag_read_data;
wire pending_hit = valid[pending_set] && tag_read_data == pending_tag;
wire drain_last = drain_word == 15;
wire tag_write_enable = state == ST_LINE_DRAIN && drain_last;

__CACHE_TAGS__ u_tags (
    .clk(clk),
    .write_enable(tag_write_enable),
    .address(pending_set),
    .write_data(pending_tag),
    .read_data(tag_read_data)
);

wire [31:0] drain_beat = refill_buffer[drain_word[3:1]];
wire [15:0] drain_data = drain_word[0] ? drain_beat[31:16] : drain_beat[15:0];
wire drain_write = state == ST_LINE_DRAIN;
wire hit_write = state == ST_CHECK && pending_write && pending_hit;
wire cache_write_enable = drain_write || hit_write;
wire [9:0] cache_write_address = drain_write ?
    {pending_set, drain_word} : pending_address[9:0];
wire [15:0] cache_write_data = drain_write ? drain_data : pending_write_data;
wire [9:0] cache_read_address = state == ST_IDLE ?
    cpu_address[9:0] : pending_address[9:0];
wire [15:0] cache_read_data;
wire [15:0] unused_cache_rw_data;

__CACHE_DATA__ u_data (
    .clk(clk),
    .read_address(cache_read_address),
    .rw_write_enable(cache_write_enable),
    .rw_address(cache_write_address),
    .rw_write_data(cache_write_data),
    .read_data(cache_read_data),
    .rw_read_data(unused_cache_rw_data)
);

assign cpu_request_ready = state == ST_IDLE;
assign cpu_response_valid = state == ST_CPU_RESPONSE;
assign cpu_read_data = response_data;
assign cpu_error = state == ST_CPU_RESPONSE && response_error;
assign memory_request_valid = state == ST_WORD_REQUEST || state == ST_LINE_REQUEST;
assign memory_write = pending_write;
assign memory_address = pending_write ? pending_address[21:0] :
                        {pending_address[21:4], 4'b0};
assign memory_write_data = pending_write_data;
assign memory_response_ready = state == ST_WORD_RESPONSE || state == ST_LINE_RECEIVE;

always @(posedge clk) begin
    if (reset) begin
        state <= ST_IDLE;
        valid <= __INITIAL_VALID__;
        response_error <= 0;
    end else begin
        case (state)
            ST_IDLE: if (cpu_request_valid) begin
                pending_write <= cpu_write;
                pending_address <= cpu_address;
                pending_write_data <= cpu_write_data;
                response_error <= 0;
                if (!cpu_address_valid) begin
                    response_data <= 0;
                    response_error <= 1;
                    state <= ST_CPU_RESPONSE;
                end else begin
                    state <= ST_CHECK;
                end
            end

            ST_CHECK: begin
                if (pending_write) begin
                    state <= ST_WORD_REQUEST;
                end else if (pending_hit) begin
                    response_data <= cache_read_data;
                    state <= ST_CPU_RESPONSE;
                end else begin
                    refill_beat <= 0;
                    state <= ST_LINE_REQUEST;
                end
            end

            ST_WORD_REQUEST: if (memory_request_ready)
                state <= ST_WORD_RESPONSE;

            ST_WORD_RESPONSE: if (memory_response_valid) begin
                response_data <= 0;
                response_error <= memory_error;
                state <= ST_CPU_RESPONSE;
            end

            ST_LINE_REQUEST: if (memory_request_ready) begin
                refill_beat <= 0;
                state <= ST_LINE_RECEIVE;
            end

            ST_LINE_RECEIVE: if (memory_response_valid) begin
                if (memory_error) begin
                    response_data <= 0;
                    response_error <= 1;
                    state <= ST_CPU_RESPONSE;
                end else begin
                    refill_buffer[refill_beat] <= memory_read_data;
                    if (refill_beat == 7) begin
                        drain_word <= 0;
                        state <= ST_LINE_DRAIN;
                    end else begin
                        refill_beat <= refill_beat + 1'b1;
                    end
                end
            end

            ST_LINE_DRAIN: begin
                if (drain_word == pending_word)
                    response_data <= drain_data;
                if (drain_last) begin
                    valid[pending_set] <= 1;
                    state <= ST_CPU_RESPONSE;
                end else begin
                    drain_word <= drain_word + 1'b1;
                end
            end

            ST_CPU_RESPONSE: if (cpu_response_ready)
                state <= ST_IDLE;

            default: state <= ST_IDLE;
        endcase

        if (invalidate_all)
            valid <= 0;
    end
end

endmodule
