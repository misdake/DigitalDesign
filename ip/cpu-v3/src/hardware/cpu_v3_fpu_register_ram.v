module CpuV3FpuRegisterRam (
    input wire clk,
    input wire write_enable,
    input wire [5:0] write_address,
    input wire [15:0] write_data,
    input wire [5:0] read_a_address,
    input wire [5:0] read_b_address,
    output wire [15:0] read_a_data,
    output wire [15:0] read_b_data
);

// Sixteen four-lane fix16 vectors in distributed RAM: one synchronous write
// port and two asynchronous read ports, so FPU execution reads operands
// combinationally without a staging pass.
reg [15:0] words [0:63];
integer initial_word;
initial begin
    for (initial_word = 0; initial_word < 64; initial_word = initial_word + 1)
        words[initial_word] = 0;
end

always @(posedge clk) begin
    if (write_enable)
        words[write_address] <= write_data;
end

assign read_a_data = words[read_a_address];
assign read_b_data = words[read_b_address];

endmodule
