`timescale 1ns/1ps
module tb;
reg clk=0;
reg [1:0] buttons=0;
wire [5:0] leds;
wire uart_tx;
LightingBoard dut(.*);
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
always #18.5185 clk=~clk;
task restart;
begin buttons=1;repeat(8) @(negedge clk);buttons=0;repeat(80) @(negedge clk);end
endtask
task verdict;
input [7:0] wanted;
integer cycles;
begin
 cycles=0;
 while(!dut.done && cycles<200000) begin @(negedge clk);cycles=cycles+1;end
 if(!dut.done || dut.status!==wanted) $fatal(1,"verdict got=%d wanted=%d checked=%d",dut.status,wanted,dut.checked_outputs);
end
endtask
task read_byte;
output [7:0] value;
integer j;
begin
 @(negedge uart_tx);repeat(351) @(posedge clk);
 for(j=0;j<8;j=j+1) begin value[j]=uart_tx;repeat(234) @(posedge clk);end
end
endtask
reg [7:0] bytes[0:7];
integer i;
initial begin
 verdict(0);
 if(dut.checked_outputs!==1408) $fatal(1,"incomplete fixture traversal");
 for(i=0;i<8;i=i+1) read_byte(bytes[i]);
 if(bytes[0]!==8'h44 || bytes[1]!==8'h44 || bytes[2]!==8'h48 || bytes[3]!==8'h54 ||
    bytes[4]!==1 || bytes[5]!==8'h0c || bytes[6]!==0 || bytes[7]!==8'h11) $fatal(1,"UART success frame");
 // Mid-stream user reset must restart the complete scoreboard and flush stale tokens.
 restart();while(dut.send_index<4) @(negedge clk);restart();verdict(0);
 if(dut.checked_outputs!==1408) $fatal(1,"reset did not restart coverage");
 // Negative controls prove that reporting cannot pass by replaying canned goldens.
 restart();force dut.out_g=9'h1ff;verdict(1);release dut.out_g;
 restart();force dut.out_id=0;verdict(2);release dut.out_id;
 restart();force dut.out_valid=0;verdict(3);release dut.out_valid;
 for(i=0;i<8;i=i+1) read_byte(bytes[i]);
 if(bytes[6]!==3 || bytes[7]!==8'h12) $fatal(1,"UART failure frame");
 $display("DIGITAL_DESIGN_PASS: 1408 outputs, reset, numeric/id/timeout negative controls, UART");$finish;
end
initial begin repeat(1000000) @(posedge clk);$fatal(1,"bounded board simulation timeout");end
endmodule
