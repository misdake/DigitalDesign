module tb;
reg clk = 0;
reg report_enable = 0;
reg [7:0] status = 0;
wire uart_tx;
wire uart_busy;
wire frame_toggle;

{{ module_name }} dut(
    .clk(clk),
    .report_enable(report_enable),
    .status(status),
    .uart_tx(uart_tx),
    .uart_busy(uart_busy),
    .frame_toggle(frame_toggle)
);

always #1 clk = ~clk;

task read_byte;
    output [7:0] value;
    integer bit_index;
    begin
        @(negedge uart_tx);
        repeat ({{ clocks_per_bit }} + {{ clocks_per_bit / 2 }}) @(posedge clk);
        for (bit_index = 0; bit_index < 8; bit_index = bit_index + 1) begin
            value[bit_index] = uart_tx;
            repeat ({{ clocks_per_bit }}) @(posedge clk);
        end
    end
endtask

reg [7:0] received [0:7];
integer i;
initial begin
    repeat (2) @(posedge clk);
    status = 8'h35;
    report_enable = 1;
    for (i = 0; i < 8; i = i + 1)
        read_byte(received[i]);
    wait (uart_busy == 0);
    #1;
    if (received[0] !== 8'h44 || received[1] !== 8'h44 ||
        received[2] !== 8'h48 || received[3] !== 8'h54 ||
        received[4] !== 8'h01 || received[5] !== 8'h{{ "{:02x}"|format(test_id) }} ||
        received[6] !== 8'h35 || received[7] !== (8'h{{ "{:02x}"|format(checksum_base) }} ^ 8'h35))
        $fatal(1, "bad first diagnostic frame");
    if (frame_toggle !== 1)
        $fatal(1, "frame completion did not toggle");

    status = 8'ha6;
    for (i = 0; i < 8; i = i + 1)
        read_byte(received[i]);
    wait (uart_busy == 0);
    #1;
    if (received[6] !== 8'ha6 || received[7] !== (8'h{{ "{:02x}"|format(checksum_base) }} ^ 8'ha6))
        $fatal(1, "repeated frame did not latch the new status");
    if (frame_toggle !== 0)
        $fatal(1, "second frame completion did not toggle");

    $display("DIGITAL_DESIGN_PASS");
    $finish;
end

initial begin
    repeat (2_000) @(posedge clk);
    $fatal(1, "timeout");
end
endmodule
