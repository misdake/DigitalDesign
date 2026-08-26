module DisplayLineBuffer(
    input wire write_clock, input wire write_enable,
    input wire [8:0] write_address, input wire [31:0] write_data,
    input wire read_clock, input wire [8:0] read_address,
    output reg [31:0] read_data = 0
);
// 480x32 = 15360 bits. Gowin maps this dual-clock RAM into one 18-Kbit BSRAM.
reg [31:0] memory [0:511];
always @(posedge write_clock)
    if (write_enable) memory[write_address] <= write_data;
always @(posedge read_clock)
    read_data <= memory[read_address];
endmodule
