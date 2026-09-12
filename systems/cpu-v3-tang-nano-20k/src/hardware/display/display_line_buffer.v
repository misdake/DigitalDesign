module DisplayLineBuffer(
    input wire write_clock, input wire write_enable,
    input wire [9:0] write_address, input wire [31:0] write_data,
    input wire read_clock, input wire [9:0] read_address,
    output reg [31:0] read_data = 0
);
// Two 400-pixel lines pack into 400 32-bit words = 12800 bits, which fits one
// 18432-bit block, so Gowin maps this dual-clock RAM into a single 18-Kbit
// BSRAM. The 512-word depth is the next power of two above the 400 live
// addresses; the two slot bases are 0 and 200 words.
reg [31:0] memory [0:511];
always @(posedge write_clock)
    if (write_enable) memory[write_address] <= write_data;
always @(posedge read_clock)
    read_data <= memory[read_address];
endmodule
