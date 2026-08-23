module {{ module_name }}(
    input wire clk,
    input wire a_write_enable,
    input wire [9:0] a_address,
    input wire [{{ high_bit }}:0] a_write_data,
    output reg [{{ high_bit }}:0] a_read_data,
    input wire b_write_enable,
    input wire [9:0] b_address,
    input wire [{{ high_bit }}:0] b_write_data,
    output reg [{{ high_bit }}:0] b_read_data
);

reg [{{ high_bit }}:0] memory [0:1023];
integer init_address;

initial begin
    for (init_address = 0; init_address < 1024; init_address = init_address + 1)
        memory[init_address] = {{ image.default_literal }};
{% for word in image.overrides %}    memory[{{ word.address }}] = {{ word.literal }};
{% endfor %}end

always @(posedge clk) begin
    if (a_write_enable)
        memory[a_address] <= a_write_data;
    else
        a_read_data <= memory[a_address];
end

always @(posedge clk) begin
    if (b_write_enable)
        memory[b_address] <= b_write_data;
    else
        b_read_data <= memory[b_address];
end

endmodule
