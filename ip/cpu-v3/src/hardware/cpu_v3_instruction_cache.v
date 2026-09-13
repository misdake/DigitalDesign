module CpuV3InstructionCache (
    input wire clk, input wire reset, input wire invalidate_all,
    input wire cpu_request_valid, input wire [31:0] cpu_address,
    input wire cpu_response_ready,
    input wire memory_request_ready, input wire memory_response_valid,
    input wire [63:0] memory_read_data, input wire memory_error,
    output wire cpu_request_ready, output wire cpu_response_valid,
    output wire [15:0] cpu_read_data, output wire cpu_error,
    output wire memory_request_valid, output wire [21:0] memory_address,
    output wire memory_response_ready
);
__CACHE__ u_cache (
    .clk(clk), .reset(reset), .invalidate_all(invalidate_all),
    .cpu_request_valid(cpu_request_valid), .cpu_write(1'b0),
    .cpu_address(cpu_address), .cpu_write_data(16'b0),
    .cpu_response_ready(cpu_response_ready),
    .memory_request_ready(memory_request_ready),
    .memory_response_valid(memory_response_valid),
    .memory_read_data(memory_read_data), .memory_error(memory_error),
    .cpu_request_ready(cpu_request_ready),
    .cpu_response_valid(cpu_response_valid), .cpu_read_data(cpu_read_data),
    .cpu_error(cpu_error), .memory_request_valid(memory_request_valid),
    .memory_write(), .memory_line(), .memory_address(memory_address),
    .memory_write_data(), .memory_response_ready(memory_response_ready)
);
endmodule
