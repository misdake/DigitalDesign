module FetchPipelineProbe (
    input wire clk,
    input wire reset,
    output wire halted,
    output wire fault,
    output wire [15:0] halt_signal,
    output wire [31:0] retired_words
);

wire core_request_valid;
wire [31:0] core_address;
wire core_response_ready;
wire core_request_ready;
wire core_response_valid;
wire [15:0] core_read_data;
wire core_error;
wire memory_request_valid;
wire [31:0] memory_address;
wire memory_response_ready;
reg memory_response_valid = 0;
reg [15:0] memory_read_data = 0;

wire [15:0] instruction_word = memory_address[15:0] < 8 ?
    16'ha001 : 16'he800;

always @(posedge clk) begin
    memory_response_valid <= memory_request_valid;
    if (memory_request_valid)
        memory_read_data <= instruction_word;
end

__FETCH_QUEUE__ u_fetch (
    .clk(clk),
    .reset(reset),
    .flush(halted || fault),
    .core_request_valid(core_request_valid),
    .core_address(core_address),
    .core_response_ready(core_response_ready),
    .memory_request_ready(1'b1),
    .memory_response_valid(memory_response_valid),
    .memory_read_data(memory_read_data),
    .memory_error(1'b0),
    .core_request_ready(core_request_ready),
    .core_response_valid(core_response_valid),
    .core_read_data(core_read_data),
    .core_error(core_error),
    .memory_request_valid(memory_request_valid),
    .memory_address(memory_address),
    .memory_response_ready(memory_response_ready)
);

__CPU_CORE__ u_core (
    .clk(clk),
    .reset(reset),
    .instruction_request_ready(core_request_ready),
    .instruction_response_valid(core_response_valid),
    .instruction_data(core_read_data),
    .instruction_error(core_error),
    .data_request_ready(1'b1),
    .data_response_valid(1'b0),
    .data_read_data(16'b0),
    .data_error(1'b0),
    .device_read_data(16'b0),
    .instruction_request_valid(core_request_valid),
    .instruction_address(core_address),
    .instruction_response_ready(core_response_ready),
    .data_request_valid(),
    .data_write(),
    .data_address(),
    .data_write_data(),
    .data_response_ready(),
    .device_index(),
    .device_channel(),
    .device_read_enable(),
    .device_write_enable(),
    .device_write_data(),
    .halted(halted),
    .halt_signal(halt_signal),
    .fault(fault),
    .fault_code(),
    .fault_pc(),
    .pc(),
    .code_segment(),
    .data_segment(),
    .retired_words(retired_words)
);

endmodule
