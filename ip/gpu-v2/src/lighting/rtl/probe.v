// Serial anti-optimization harness for isolated PnR. No board pin ABI.
module lighting_probe(
    input clk, reset, ce, shift_en, serial_in, context_valid, in_valid, out_ready,
    output context_ready, in_ready, out_valid,
    output reg [31:0] checksum
);
reg [276:0] source;
wire [31:0] out_id;
wire [15:0] out_epoch;
wire [8:0] out_g, out_h;
always @(posedge clk) begin
    if (reset) begin source <= 0; checksum <= 0; end
    else begin
        if (shift_en) source <= {source[275:0], serial_in};
        if (ce && out_valid && out_ready)
            checksum <= {checksum[30:0], checksum[31]} ^ out_id
                ^ {out_epoch, out_g[7:0], out_h[7:0]}
                ^ {30'd0, out_g[8], out_h[8]};
    end
end
gpu_v2_lighting dut(
    .clk(clk), .reset(reset), .ce(ce),
    .context_valid(context_valid), .context_ready(context_ready),
    .in_valid(in_valid), .in_ready(in_ready),
    .out_ready(out_ready), .out_valid(out_valid),
    .context_epoch(source[15:0]), .context_mode(source[17:16]),
    .context_code(source[22:18]),
    .light_x(source[38:23]), .light_y(source[54:39]), .light_z(source[70:55]),
    .ambient(source[79:71]), .directional(source[88:80]),
    .ray_x(source[104:89]), .ray_y(source[120:105]), .ray_k(source[136:121]),
    .in_row0(source[172:137]), .in_row1(source[208:173]), .in_row2(source[244:209]),
    .in_id(source[276:245]),
    .out_id(out_id), .out_epoch(out_epoch), .out_g(out_g), .out_h(out_h)
);
endmodule
