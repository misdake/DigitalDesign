module CpuV3GprRam (
    input wire clk,
    input wire write_enable,
    input wire [3:0] write_address,
    input wire [15:0] write_data,
    input wire [3:0] read_a_address,
    input wire [3:0] read_b_address,
    output wire [15:0] read_a_data,
    output wire [15:0] read_b_data
);

// Sixteen scalar registers in distributed RAM. Gowin duplicates the four
// 16x4 RAM16 cells for the second asynchronous read port, for eight cells in
// total. Writes are synchronous and mirrored into both inferred read copies.
(* syn_ramstyle = "distributed_ram" *) reg [15:0] words [0:15];
integer initial_word;
initial begin
    for (initial_word = 0; initial_word < 16; initial_word = initial_word + 1)
        words[initial_word] = 0;
end

always @(posedge clk) begin
    if (write_enable)
        words[write_address] <= write_data;
end

assign read_a_data = words[read_a_address];
assign read_b_data = words[read_b_address];

endmodule
