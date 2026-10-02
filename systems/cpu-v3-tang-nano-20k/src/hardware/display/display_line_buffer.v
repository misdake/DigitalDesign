module DisplayLineBuffer(
    input wire write_clock, input wire write_enable,
    input wire [8:0] write_address, input wire [63:0] write_data,
    input wire read_clock, input wire [8:0] read_address_a, read_address_b,
    output reg [31:0] read_data_a = 0, read_data_b = 0
);
// Four lines occupy qword addresses 0..399. Each 512x32 bank has an
// independent pixel-clock read port. Addresses 448..511 hold identical
// 64x8 sRGB tables (one byte per word; the spare bytes stay unused).
(* syn_ramstyle = "block_ram" *) reg [31:0] memory_a [0:511];
(* syn_ramstyle = "block_ram" *) reg [31:0] memory_b [0:511];
initial begin
__SRGB_INIT__
end
always @(posedge write_clock)
    if (write_enable) begin
        memory_a[write_address] <= write_data[31:0];
        memory_b[write_address] <= write_data[63:32];
    end
always @(posedge read_clock) begin
    read_data_a <= memory_a[read_address_a];
    read_data_b <= memory_b[read_address_b];
end
endmodule
