module CpuV3FpuRegisterRam (
    input wire clk,
    input wire write_enable,
    input wire [8:0] write_address,
    input wire [31:0] write_data,
    input wire [8:0] read_a_address,
    input wire [8:0] read_b_address,
    output wire [31:0] read_a_data,
    output wire [31:0] read_b_data
);

// Two identical 512x32 storage mirrors, each written through one synchronous
// port and read through one synchronous port. No ramstyle attribute is used:
// the plain inference is meant to let Gowin map every mirror to a single SDPB
// pseudo-dual-port BSRAM (one write port plus one read port). The write port
// broadcasts the same address and data into both mirrors, read port A serves
// mirror 0 and read port B serves mirror 1. Reads have one cycle of latency
// and read-first semantics: when a write and a read hit the same address in
// one cycle, both read ports still return the old word.
//
// Address map: physical addresses 0..63 alias architectural registers
// F0..F63, and 64..511 are the hidden LUT region used by RCP/RSQRT/SINCOS.
// The parent is responsible for forcing the top three address bits of
// architectural accesses to zero; this leaf never muxes an address.
reg [31:0] mirror_0 [0:511];
reg [31:0] mirror_1 [0:511];
reg [31:0] read_a_data_r = 0;
reg [31:0] read_b_data_r = 0;
integer initial_word;
initial begin
    for (initial_word = 0; initial_word < 512; initial_word = initial_word + 1) begin
        mirror_0[initial_word] = 0;
        mirror_1[initial_word] = 0;
    end
end

always @(posedge clk) begin
    if (write_enable)
        mirror_0[write_address] <= write_data;
    read_a_data_r <= mirror_0[read_a_address];
end

always @(posedge clk) begin
    if (write_enable)
        mirror_1[write_address] <= write_data;
    read_b_data_r <= mirror_1[read_b_address];
end

assign read_a_data = read_a_data_r;
assign read_b_data = read_b_data_r;

endmodule
