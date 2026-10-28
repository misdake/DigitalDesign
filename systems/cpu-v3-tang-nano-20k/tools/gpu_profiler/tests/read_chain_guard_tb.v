// Appended to the replay's actual MC wiring, pin model and read guard.
reg read_valid=0;
reg[21:0]read_address=0;
assign roq=read_valid;
assign roa=read_address;
assign roc=2'd3;
assign frq=0;assign fwq=0;assign fra=0;assign fwa=0;
assign frc=0;assign fwc=0;assign fwd=0;

integer requests=0,terminals=0,takes=0,chains=0,pairs=0,i;
reg missing_successor;
reg[63:0]expected;
always @(posedge controller_clk)if(!reset)begin
 if(mc.bridge.take)takes=takes+1;
 if(mc.bridge.read_chain_accept)chains=chains+1;
end
always @(posedge clk)if(!reset)begin
 if(roq&&ror)begin
  requests=requests+1;
  read_address<=22'd64;
  if(requests==2)read_valid<=0;
 end
 if(rov)begin
  if(roe)$fatal(1,"Unexpected MC error");
  if(terminals==0)expected=64'h1234567812345678;
  else expected=64'h89abcdef89abcdef;
  if((!missing_successor||terminals==0)&&rod!==expected)
   $fatal(1,"Read payload mismatch: terminal=%0d data=%h expected=%h",terminals,rod,expected);
  pairs=pairs+1;
  if(rol)terminals=terminals+1;
 end
end
initial begin
 missing_successor=$test$plusargs("MISSING_SUCCESSOR");
 for(i=0;i<64;i=i+1)begin
  if(i<32||!missing_successor)begin
   if(i<32)pins.mem[physical(i)]=32'h12345678;
   else pins.mem[physical(i)]=32'h89abcdef;
   pins.marked[physical(i)]=1;
   valid_bytes[physical(i)]=15;
  end
 end
 repeat(3)@(negedge clk);reset=0;
 wait(mc.initialized);@(negedge clk);read_valid=1;
 wait(terminals==2);@(negedge clk);
 if(requests!=2||takes!=1||chains!=1||pairs!=32)
  $fatal(1,"Actual continuation required: requests=%0d take=%0d read_chain=%0d terminal=%0d pairs=%0d",requests,takes,chains,terminals,pairs);
 $display("READ_CHAIN_GUARD_PASS requests=%0d take=%0d read_chain=%0d terminal=%0d pairs=%0d",requests,takes,chains,terminals,pairs);
 $finish;
end
initial begin repeat(10000)@(negedge clk);$fatal(1,"Read chain test exceeded10000cycles");end
endmodule
