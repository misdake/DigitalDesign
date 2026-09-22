// Related-clock write gearbox for the 54 MHz system / 108 MHz SDRAM
// controller path. Four 64-bit values must be captured before write_start.
// Longer bursts may then continue capturing one value per logic clock while
// the controller consumes two 32-bit halves per logic-clock period.
module TangNano20KSdramWriteGearbox108M54M (
    input wire logic_clk,
    input wire controller_clk,
    input wire reset,
    input wire capture_valid,
    input wire [63:0] capture_data,
    input wire write_start,
    input wire [4:0] burst_length,
    output wire [31:0] controller_data
);

(* syn_ramstyle = "registers" *) reg [63:0] buffer [0:7];
reg [2:0] capture_pointer = 0;
always @(posedge logic_clk) begin
    if (reset)
        capture_pointer <= 0;
    else if (capture_valid) begin
        buffer[capture_pointer] <= capture_data;
        capture_pointer <= capture_pointer + 1'b1;
    end
end

reg active = 0;
reg [4:0] physical_beat = 0;
reg [2:0] base = 0;
wire [2:0] command_base = capture_pointer - 3'd4;
wire [2:0] pair_index = base + physical_beat[3:1];
wire [63:0] pair = active ? buffer[pair_index] : buffer[command_base];
assign controller_data = physical_beat[0] ? pair[63:32] : pair[31:0];

always @(posedge controller_clk) begin
    if (reset) begin
        active <= 0;
        physical_beat <= 0;
        base <= 0;
    end else if (write_start) begin
        // M0 is sampled on this edge from command_base. The next edge must
        // present M1 from the same 64-bit value.
        active <= burst_length != 0;
        physical_beat <= burst_length == 0 ? 0 : 1;
        base <= command_base;
    end else if (active) begin
        if (physical_beat == burst_length) begin
            active <= 0;
            physical_beat <= 0;
        end else begin
            physical_beat <= physical_beat + 1'b1;
        end
    end
end

endmodule
