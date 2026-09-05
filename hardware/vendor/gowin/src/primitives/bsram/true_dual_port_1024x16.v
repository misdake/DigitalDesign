module {{ module_name }}(
    input wire clk,
    input wire a_write_enable,
    input wire [9:0] a_address,
    input wire [15:0] a_write_data,
    output wire [15:0] a_read_data,
    input wire b_write_enable,
    input wire [9:0] b_address,
    input wire [15:0] b_write_data,
    output wire [15:0] b_read_data
);

`ifdef __ICARUS__
reg [15:0] memory [0:1023];
reg [15:0] a_read_data_reg;
reg [15:0] b_read_data_reg;
integer init_address;

assign a_read_data = a_read_data_reg;
assign b_read_data = b_read_data_reg;

initial begin
    a_read_data_reg = 16'b0;
    b_read_data_reg = 16'b0;
    for (init_address = 0; init_address < 1024; init_address = init_address + 1)
        memory[init_address] = {{ image.default_literal }};
{% for word in image.overrides %}    memory[{{ word.address }}] = {{ word.literal }};
{% endfor %}end

always @(posedge clk) begin
    if (a_write_enable)
        memory[a_address] <= a_write_data;
    else
        a_read_data_reg <= memory[a_address];
end

always @(posedge clk) begin
    if (b_write_enable)
        memory[b_address] <= b_write_data;
    else
        b_read_data_reg <= memory[b_address];
end
`else
DPB #(
    .READ_MODE0(1'b0),
    .READ_MODE1(1'b0),
    .WRITE_MODE0(2'b00),
    .WRITE_MODE1(2'b00),
    .BIT_WIDTH_0(16),
    .BIT_WIDTH_1(16),
    .BLK_SEL_0(3'b000),
    .BLK_SEL_1(3'b000),
{% for chunk in init_chunks %}    .INIT_RAM_{{ chunk.index }}(256'h{{ chunk.literal }}),
{% endfor %}    .RESET_MODE("SYNC")
) memory (
    .DOA(a_read_data),
    .DOB(b_read_data),
    .DIA(a_write_data),
    .DIB(b_write_data),
    .BLKSELA(3'b000),
    .BLKSELB(3'b000),
    .ADA({a_address, 2'b00, 2'b11}),
    .ADB({b_address, 2'b00, 2'b11}),
    .WREA(a_write_enable),
    .WREB(b_write_enable),
    .CLKA(clk),
    .CLKB(clk),
    .CEA(1'b1),
    .CEB(1'b1),
    .OCEA(1'b0),
    .OCEB(1'b0),
    .RESETA(1'b0),
    .RESETB(1'b0)
);
`endif

endmodule
