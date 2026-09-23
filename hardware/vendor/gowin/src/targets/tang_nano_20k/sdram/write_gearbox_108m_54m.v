// Related-clock write gearbox for the 54 MHz system / 108 MHz SDRAM
// controller path. The controller samples a low/high 32-bit pair during each
// 54-MHz period. One 64-bit holding register cuts the source's combinational
// path to the 108-MHz controller; the rest of a long write remains in the
// source cache entry. WRITE commands are phase-aligned so their low half is
// sampled on the logic-clock falling edge. The following high half is sampled
// on the rising edge, when the source may replace the holding register.
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

reg [63:0] pair = 0;
reg active = 0;
reg [4:0] physical_beat = 0;
reg high_phase = 0;

assign controller_data = high_phase ? pair[63:32] : pair[31:0];
// During a burst, high_phase is already high before the shared controller/
// logic rising edge. Both domains therefore sample the old high half while the
// logic domain registers the next pair for the following falling-edge sample.
// The logic-domain adapter schedules capture_valid only for the preload beat
// and for the rising edges that consume a high half. No controller-domain
// ready signal feeds back into the 54-MHz producer path.

always @(posedge logic_clk) begin
    if (reset) begin
        pair <= 0;
    end else begin
        if (capture_valid) begin
            pair <= capture_data;
        end
    end
end

always @(posedge controller_clk) begin
    if (reset) begin
        active <= 0;
        physical_beat <= 0;
        high_phase <= 0;
    end else if (write_start) begin
        active <= burst_length != 0;
        physical_beat <= burst_length == 0 ? 0 : 1;
        high_phase <= burst_length != 0;
    end else if (active) begin
        if (high_phase) begin
            if (physical_beat == burst_length) begin
                active <= 0;
                physical_beat <= 0;
                high_phase <= 0;
            end else begin
                physical_beat <= physical_beat + 1'b1;
                high_phase <= 0;
            end
        end else begin
            // The next registered low half is sampled directly.
            physical_beat <= physical_beat + 1'b1;
            high_phase <= 1;
        end
    end
end

endmodule
