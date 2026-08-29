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
    input wire [15:0] memory_read_data,
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

localparam [2:0] ST_IDLE = 0;
localparam [2:0] ST_CHECK = 1;
localparam [2:0] ST_MEMORY_REQUEST = 2;
localparam [2:0] ST_MEMORY_RESPONSE = 3;
localparam [2:0] ST_CPU_RESPONSE = 4;

reg [2:0] state = ST_IDLE;
reg pending_write = 0;
reg [31:0] pending_address = 0;
reg [15:0] pending_write_data = 0;
reg [3:0] fill_word = 0;
reg [15:0] response_data = 0;
reg response_error = 0;
reg [63:0] valid = __INITIAL_VALID__;

wire cpu_address_valid = cpu_address[31:22] == 0;

wire [5:0] pending_set = pending_address[9:4];
wire [11:0] pending_tag = pending_address[21:10];
wire [3:0] pending_word = pending_address[3:0];
wire [11:0] tag_read_data;
wire tag_write_enable = state == ST_MEMORY_RESPONSE && memory_response_valid &&
                        !memory_error && !pending_write && fill_word == 15;
wire pending_hit = valid[pending_set] && tag_read_data == pending_tag;

__CACHE_TAGS__ u_tags (
    .clk(clk),
    .write_enable(tag_write_enable),
    .address(pending_set),
    .write_data(pending_tag),
    .read_data(tag_read_data)
);

wire refill_write = state == ST_MEMORY_RESPONSE && memory_response_valid &&
                    !memory_error && !pending_write;
wire hit_write = state == ST_CHECK && pending_write && pending_hit;
wire cache_write_enable = refill_write || hit_write;
wire [9:0] cache_write_address = refill_write ?
    {pending_set, fill_word} : pending_address[9:0];
wire [15:0] cache_write_data = refill_write ? memory_read_data : pending_write_data;
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
assign memory_request_valid = state == ST_MEMORY_REQUEST;
assign memory_write = pending_write;
assign memory_address = pending_write ? pending_address[21:0] :
                        {pending_address[21:4], fill_word};
assign memory_write_data = pending_write_data;
assign memory_response_ready = state == ST_MEMORY_RESPONSE;

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
                    state <= ST_MEMORY_REQUEST;
                end else if (pending_hit) begin
                    response_data <= cache_read_data;
                    state <= ST_CPU_RESPONSE;
                end else begin
                    fill_word <= 0;
                    state <= ST_MEMORY_REQUEST;
                end
            end

            ST_MEMORY_REQUEST: if (memory_request_ready)
                state <= ST_MEMORY_RESPONSE;

            ST_MEMORY_RESPONSE: if (memory_response_valid) begin
                if (memory_error) begin
                    response_data <= 0;
                    response_error <= 1;
                    state <= ST_CPU_RESPONSE;
                end else if (pending_write) begin
                    response_data <= 0;
                    state <= ST_CPU_RESPONSE;
                end else begin
                    if (fill_word == pending_word)
                        response_data <= memory_read_data;
                    if (fill_word == 15) begin
                        valid[pending_set] <= 1;
                        state <= ST_CPU_RESPONSE;
                    end else begin
                        fill_word <= fill_word + 1'b1;
                        state <= ST_MEMORY_REQUEST;
                    end
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
