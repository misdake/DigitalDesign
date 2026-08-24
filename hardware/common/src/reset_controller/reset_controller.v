module {{ module_name }}(
    input wire clk,
    input wire external_reset,
    input wire clock_ready,
    output wire reset,
    output wire clock_ready_synchronized,
    output wire external_reset_seen
);

reg external_meta = 1'b0;
reg external_sync = 1'b0;
reg ready_meta = 1'b0;
reg ready_sync = 1'b0;
reg external_seen_reg = 1'b0;
reg [{{ counter_high_bit }}:0] hold_remaining = {{ counter_width }}'d{{ hold_cycles }};

always @(posedge clk) begin
    external_meta <= external_reset;
    external_sync <= external_meta;
    ready_meta <= clock_ready;
    ready_sync <= ready_meta;

    // Use the first-stage values here: after this edge they are exactly the
    // values transferred into the synchronized outputs by nonblocking assigns.
    if (external_meta)
        external_seen_reg <= 1'b1;

    if (external_meta || !ready_meta)
        hold_remaining <= {{ counter_width }}'d{{ hold_cycles }};
    else if (hold_remaining != 0)
        hold_remaining <= hold_remaining - 1'b1;
end

assign reset = external_sync || !ready_sync || hold_remaining != 0;
assign clock_ready_synchronized = ready_sync;
assign external_reset_seen = external_seen_reg;

endmodule
