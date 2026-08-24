module CpuV3CacheTagRam (
    input wire clk,
    input wire write_enable,
    input wire [5:0] address,
    input wire [11:0] write_data,
    output wire [11:0] read_data
);

reg [11:0] tags [0:63];

always @(posedge clk) begin
    if (write_enable)
        tags[address] <= write_data;
end

assign read_data = tags[address];

endmodule
