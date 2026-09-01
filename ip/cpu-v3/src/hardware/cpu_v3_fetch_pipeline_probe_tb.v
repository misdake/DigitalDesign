module tb;
reg clk = 0;
reg reset = 1;
wire halted;
wire fault;
wire [15:0] halt_signal;
wire [31:0] retired_words;
integer cycles = 0;
integer first_retire_cycle = -1;

FetchPipelineProbe dut(.*);
always #5 clk = ~clk;

always @(posedge clk) begin
    cycles <= cycles + 1;
    if (first_retire_cycle < 0 && retired_words == 1)
        first_retire_cycle <= cycles;
    if (cycles > 200)
        $fatal(1, "fetch pipeline probe exceeded cycle limit");
end

initial begin
    repeat (3) @(posedge clk);
    reset <= 0;
    while (!halted && !fault) @(posedge clk);
    #1;
    if (fault || halt_signal != 8 || retired_words != 9)
        $fatal(1, "sequential ALU stream failed: fault=%0d signal=%0d retired=%0d",
               fault, halt_signal, retired_words);
    if (cycles - first_retire_cycle > 17)
        $fatal(1, "eight post-first-retire instructions took %0d cycles, expected <=17",
               cycles - first_retire_cycle);
    $display("DIGITAL_DESIGN_PASS");
    $finish;
end
endmodule
