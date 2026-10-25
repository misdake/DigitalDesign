module tb;
reg clk=0; always #5 clk=~clk;
reg abort=0,in_valid=0;
reg signed [35:0] in_a=0,in_b=0;
reg [8:0] in_tag=0;
wire out_valid,ordinary_pending;
wire signed [63:0] out_product;
wire [8:0] out_tag;
CpuV3FpuExecMul dut(.*);
integer i,seed=32'h519184a,cycles=0;
reg signed [71:0] expected;
always @(posedge clk)begin
 cycles=cycles+1;
 if(cycles>4000)$fatal(1,"single-product watchdog");
end
initial begin
 for(i=0;i<2400;i=i+1)begin
  @(negedge clk);
  in_valid=i%5!=0;abort=i%17==0;
  in_a=$signed($random(seed));in_b=$signed($random(seed));
  if(i%11==0)in_b=36'h0a2f9836e;
  in_tag=$random(seed);expected=in_a*in_b;
  @(posedge clk);#1;
  if(out_valid!==(in_valid&&!abort))$fatal(1,"single-product validity");
  if(out_valid&&(out_product!==expected[63:0]||out_tag!==in_tag))$fatal(1,"single-product result/tag");
  if(ordinary_pending!==(out_valid&&in_tag[8:7]==0))$fatal(1,"single-product ownership");
 end
 $display("DIGITAL_DESIGN_PASS");$finish;
end
endmodule
