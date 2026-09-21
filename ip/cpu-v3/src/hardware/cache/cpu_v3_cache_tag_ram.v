module CpuV3CacheTagRam (
    input wire clk,
    input wire write_enable,
    input wire write_way,
    input wire [5:0] address,
    input wire [11:0] write_data,
    output wire [11:0] way_0_read_data,
    output wire [11:0] way_1_read_data
);

reg [11:0] way_0_tags [0:63];
reg [11:0] way_1_tags [0:63];
integer initial_set;
initial begin
    for (initial_set = 0; initial_set < 64; initial_set = initial_set + 1) begin
        way_0_tags[initial_set] = 0;
        way_1_tags[initial_set] = 0;
    end
end

always @(posedge clk) begin
    if (write_enable && !write_way)
        way_0_tags[address] <= write_data;
    if (write_enable && write_way)
        way_1_tags[address] <= write_data;
end

assign way_0_read_data = way_0_tags[address];
assign way_1_read_data = way_1_tags[address];

endmodule
