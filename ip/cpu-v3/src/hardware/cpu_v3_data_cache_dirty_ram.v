module CpuV3DataCacheDirtyRam (
    input wire clk,
    input wire write_enable,
    input wire write_way,
    input wire [5:0] write_set,
    input wire write_value,
    input wire clear_all,
    output wire [63:0] way_0,
    output wire [63:0] way_1
);

(* syn_ramstyle = "distributed_ram" *) reg [63:0] dirty [0:1];
assign way_0 = dirty[0];
assign way_1 = dirty[1];

always @(posedge clk) begin
    if (clear_all) begin
        dirty[0] <= 0;
        dirty[1] <= 0;
    end else if (write_enable) begin
        dirty[write_way][write_set] <= write_value;
    end
end

endmodule
