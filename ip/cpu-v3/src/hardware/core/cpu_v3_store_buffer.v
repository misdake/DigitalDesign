module CpuV3StoreBuffer (
    input wire clk,
    input wire write_enable,
    input wire [1:0] write_address,
    input wire [31:0] write_data,
    input wire [1:0] read_address,
    output wire [31:0] read_data
);
// Capture one complete F register per edge. Only four rows are live, but
// distributed RAM allocates eight 16x4 cells. Pending ownership prevents
// consumption before capture; reset invalidates ownership rather than RAM.
(* syn_ramstyle = "distributed_ram" *) reg [31:0] words [0:15];
always @(posedge clk) begin
    if (write_enable)
        words[{2'b00, write_address}] <= write_data;
end
assign read_data = words[{2'b00, read_address}];
endmodule
