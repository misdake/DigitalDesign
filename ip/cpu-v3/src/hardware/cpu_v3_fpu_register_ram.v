module CpuV3FpuRegisterRam (
    input wire clk,
    input wire [3:0] write_enable,
    input wire [3:0] write_address,
    input wire [63:0] write_data,
    input wire [3:0] read_a_address,
    input wire [3:0] read_b_address,
    output wire [63:0] read_a_data,
    output wire [63:0] read_b_data
);

// Sixteen four-lane fix16 vectors in distributed RAM: one synchronous write
// port with per-lane write enables and two asynchronous read ports, each a
// full 64-bit vector wide. The explicit style prevents registered issue
// addresses in the parent from silently remapping this leaf to BSRAM.
(* syn_ramstyle = "distributed_ram" *) reg [63:0] words [0:15];
integer initial_word;
initial begin
    for (initial_word = 0; initial_word < 16; initial_word = initial_word + 1)
        words[initial_word] = 0;
end

// Each lane spans sixteen bits, so a lane write enable maps to a group of
// four RAM16X4 cells and partial writes stay within one vector.
always @(posedge clk) begin
    if (write_enable[0])
        words[write_address][15:0] <= write_data[15:0];
    if (write_enable[1])
        words[write_address][31:16] <= write_data[31:16];
    if (write_enable[2])
        words[write_address][47:32] <= write_data[47:32];
    if (write_enable[3])
        words[write_address][63:48] <= write_data[63:48];
end

assign read_a_data = words[read_a_address];
assign read_b_data = words[read_b_address];

endmodule
