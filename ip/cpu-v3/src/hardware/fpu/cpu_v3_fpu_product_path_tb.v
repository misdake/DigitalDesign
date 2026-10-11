module tb;
reg clk=0, abort=0, instr_complete=0;
reg [3:0] instr_opcode=0;
reg [15:0] word1_raw=0;
reg [5:0] base_a=0, base_b=0;
reg [31:0] old_a=0,old_b=0,new_a=0,new_b=0;
reg [31:0] old_rf[0:511],new_rf[0:511];
wire mp_busy,dp_busy,new_busy;
wire [8:0] ma,mb,da,db,na,nb;
wire mi,di,ni,mwe,dwe,nwe;
wire signed [35:0] mia,mib,dia,dib,nia,nib;
wire [8:0] mit,dit,nit,mwa,dwa,nwa;
wire [31:0] mwd,dwd,nwd;
wire [63:0] old_acc,new_acc;
wire ov,nv;
wire signed [63:0] op,np;
wire [8:0] ot,nt;
wire [8:0] oa=dp_busy ? da : (mp_busy ? ma : 9'd0);
wire [8:0] ob=dp_busy ? db : (mp_busy ? mb : 9'd0);
wire owe=mwe || dwe;
wire [8:0] owa=mwe ? mwa : dwa;
wire [31:0] owd=mwe ? mwd : dwd;
CpuV3FpuMultiplyPath old_mul(.clk(clk),.abort(abort),.instr_complete(instr_complete),
 .instr_opcode(instr_opcode),.word1_raw(word1_raw),.base_a(base_a),.base_b(base_b),
 .rf_read_a_data(old_a),.rf_read_b_data(old_b),.rf_read_a_address(ma),.rf_read_b_address(mb),
 .mul_in_valid(mi),.mul_in_a(mia),.mul_in_b(mib),.mul_in_tag(mit),
 .mul_out_valid(ov),.mul_out_product(op),.mul_out_tag(ot),
 .rf_write_enable(mwe),.rf_write_address(mwa),.rf_write_data(mwd),.busy(mp_busy));
CpuV3FpuDotPath old_dot(.clk(clk),.abort(abort),.instr_complete(instr_complete),
 .instr_opcode(instr_opcode),.word1_raw(word1_raw),.base_a(base_a),.base_b(base_b),
 .rf_read_a_data(old_a),.rf_read_b_data(old_b),.rf_read_a_address(da),.rf_read_b_address(db),
 .mul_in_valid(di),.mul_in_a(dia),.mul_in_b(dib),.mul_in_tag(dit),
 .mul_out_valid(ov),.mul_out_product(op),.mul_out_tag(ot),
 .rf_write_enable(dwe),.rf_write_address(dwa),.rf_write_data(dwd),.busy(dp_busy),.acc_out(old_acc));
CpuV3FpuProductPath candidate(.clk(clk),.abort(abort),.instr_complete(instr_complete),
 .instr_opcode(instr_opcode),.word1_raw(word1_raw),.base_a(base_a),.base_b(base_b),
 .rf_read_a_data(new_a),.rf_read_b_data(new_b),.rf_read_a_address(na),.rf_read_b_address(nb),
 .mul_in_valid(ni),.mul_in_a(nia),.mul_in_b(nib),.mul_in_tag(nit),
 .mul_out_valid(nv),.mul_out_product(np),.mul_out_tag(nt),
 .rf_write_enable(nwe),.rf_write_address(nwa),.rf_write_data(nwd),.busy(new_busy),.acc_out(new_acc));
CpuV3FpuMulPipe old_pipe(.clk(clk),.abort(abort),.in_valid(mp_busy ? mi : di),
 .in_a(mp_busy ? mia : dia),.in_b(mp_busy ? mib : dib),.in_tag(mp_busy ? mit : dit),
 .out_valid(ov),.out_product(op),.out_tag(ot));
CpuV3FpuMulPipe new_pipe(.clk(clk),.abort(abort),.in_valid(ni),
 .in_a(nia),.in_b(nib),.in_tag(nit),.out_valid(nv),.out_product(np),.out_tag(nt));
integer cycles=0,i,mode,len,stride,age,alias_case,cases=0;
always @(posedge clk) begin
 old_a<=old_rf[oa];old_b<=old_rf[ob];new_a<=new_rf[na];new_b<=new_rf[nb];
 if(owe) old_rf[owa]<=owd;
 if(nwe) new_rf[nwa]<=nwd;
end
task check;
 begin
  if(new_busy !== (mp_busy || dp_busy) || na!==oa || nb!==ob || nwe!==owe || new_acc!==old_acc)
   $fatal(1,"control/ACC mismatch cycle=%0d case=%0d",cycles,cases);
  if(nwe && (nwa!==owa || nwd!==owd)) $fatal(1,"write mismatch cycle=%0d",cycles);
  if(ni !== (mp_busy ? mi : di)) $fatal(1,"issue mismatch");
  if(ni && (nia!==mia || nib!==mib || nit!==(mp_busy ? mit : dit)))
   $fatal(1,"operand/tag mismatch cycle=%0d",cycles);
  for(i=0;i<512;i=i+1) if(new_rf[i]!==old_rf[i]) $fatal(1,"RF mismatch at%0d",i);
 end
endtask
task step;
 input enabled;
 begin
  cycles=cycles+1;if(cycles>100000)$fatal(1,"bounded ProductPath timeout");
  #1;check;
  #3;if(enabled)clk=1;
  #1;check;
  #4;clk=0;
 end
endtask
task run_case;
 input integer family,length,bstride,abort_age,aliasing;
 integer k,sub;
 begin
  cases=cases+1;
  while(new_busy)step(1);
  base_a=aliasing==0 ? 6'd5 : 6'd62;
  base_b=aliasing==0 ? 6'd19 : 6'd63;
  sub=family==0 || family==5 ? 2 : family==1 ? 3 : family==2 ? 13 : family==3 ? 14 : 15;
  instr_opcode=family==5 ? 4'hD : 4'hC;
  word1_raw=family==5 ? {6'd61,6'd2,4'd0} :
      {6'd61,length[1:0],sub[4:0],1'b0,bstride[1:0]};
  if(aliasing==0)word1_raw[15:10]=6'd8;
  instr_complete=1;abort=abort_age==0;step(1);instr_complete=0;
  for(k=1;k<13;k=k+1)begin
   abort=k==abort_age;
   // No edge during CE0: ownership, pipe, RF and ACC must remain frozen.
   if(k%3==1)begin step(0);step(0);end
   step(1);
  end
  abort=0;step(1);
  if(new_busy)$fatal(1,"issuer did not drain");
 end
endtask
initial begin
 for(i=0;i<512;i=i+1)begin
  case(i%8)
   0:old_rf[i]=32'h80000000;1:old_rf[i]=32'h7fffffff;
   2:old_rf[i]=32'hffff8000;3:old_rf[i]=32'h00008000;
   default:old_rf[i]=32'h13579bdf ^ (i*32'h217311);
  endcase
  new_rf[i]=old_rf[i];
 end
 for(mode=0;mode<6;mode=mode+1)
  for(len=0;len<4;len=len+1)
   for(stride=0;stride<4;stride=stride+1)
    for(alias_case=0;alias_case<2;alias_case=alias_case+1)
     for(age=-1;age<10;age=age+1)run_case(mode,len,stride,age,alias_case);
 $display("DIGITAL_DESIGN_PASS ProductPath cases=%0d cycles=%0d",cases,cycles);$finish;
end
endmodule
