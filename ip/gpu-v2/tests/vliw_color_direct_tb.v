`timescale 1ns/1ps
module tb;
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0,reset=1,ce=0,proposal_ce=0,in_valid=0,out_ready=0;
reg[71:0]in_payload=0;reg[15:0]in_tex0=0,in_tex1=0,in_tex2=0,in_tex3=0;
wire gr,gv,gf,gb,cr,cv,cf,cb;wire[5:0]gk,ck;wire[7:0]grgb[0:2],crgb[0:2];
gold g(clk,reset,ce,proposal_ce,gb,in_valid,in_payload,in_tex0,in_tex1,in_tex2,in_tex3,gr,out_ready,gv,gk,grgb[0],grgb[1],grgb[2],gf);
candidate c(clk,reset,ce,proposal_ce,cb,in_valid,in_payload,in_tex0,in_tex1,in_tex2,in_tex3,cr,out_ready,cv,ck,crgb[0],crgb[1],crgb[2],cf);
reg[29:0]ga[0:8191],ca[0:8191];integer gn=0,cn=0,n=0,expected_pixels=0,cycle,epoch,k,total=0;reg[31:0]rng=32'h7654f001;reg pending_second;
task offer;
begin
 in_payload=0;in_payload[7:4]=4'd5;
 in_payload[28+:36]={rng[8:0],rng[17:9],rng[26:18],9'd111};
 in_payload[69:66]=n[5:2];in_payload[71:70]=n[1:0];
 in_payload[64]=!pending_second;
 in_payload[65]=pending_second||!n[1];
 in_tex0=rng[15:0];in_tex1=rng[31:16];in_tex2=~rng[15:0];in_tex3=rng[15:0]^16'h5555;
end endtask
initial begin
 for(epoch=0;epoch<3;epoch=epoch+1)begin
  reset=1;ce=0;proposal_ce=0;in_valid=0;out_ready=0;n=0;gn=0;cn=0;expected_pixels=0;pending_second=0;
  #5;clk=1;#5;clk=0;reset=0;offer();
  for(cycle=0;cycle<18000;cycle=cycle+1)begin
   rng={rng[30:0],rng[31]^rng[21]^rng[1]^rng[0]};
   ce=rng[3:0]!=0&&!(cycle%101>=41&&cycle%101<58);proposal_ce=ce;out_ready=ce;
   // Hold the full tap group until its second-edge acceptance.
   in_valid=n<2048;
   #4;
   if(gr!==cr||gf!==cf||gb!==cb)$fatal(1,"protocol mismatch n%0d",n);
   if(ce&&gv)begin ga[gn]={gk,grgb[2],grgb[1],grgb[0]};gn=gn+1;end
   if(ce&&cv)begin ca[cn]={ck,crgb[2],crgb[1],crgb[0]};cn=cn+1;end
   k=gn<cn?gn:cn;
   if(k>0&&ga[k-1]!==ca[k-1])$fatal(1,"result mismatch %0d",k-1);
   if(in_valid&&gr)begin
    if(in_payload[65])expected_pixels=expected_pixels+1;
    pending_second=!in_payload[65];n=n+1;
    clk=1;#5;clk=0;offer();
   end else begin clk=1;#5;clk=0;end
   if(n==2048&&gn==expected_pixels&&cn==expected_pixels)cycle=18000;
  end
  if(n!=2048||gn!=expected_pixels||cn!=expected_pixels)$fatal(1,"bounded drain %0d %0d %0d expected%0d",n,gn,cn,expected_pixels);
  for(k=0;k<expected_pixels;k=k+1)if(ga[k]!==ca[k])$fatal(1,"final result sequence mismatch %0d",k);
  total=total+gn;
  // Invalid packets retain the generic sticky fault behavior.
  in_valid=1;ce=1;proposal_ce=1;out_ready=1;in_payload[7:4]=11;
  #5;clk=1;#5;clk=0;#4;
  if(!gb||!cb)$fatal(1,"missing packet preflight");
  clk=1;#5;clk=0;#4;
  if(!gf||!cf||gv||cv)$fatal(1,"missing sticky packet fault");
  reset=1;#5;clk=1;#5;clk=0;reset=0;in_payload[7:4]=5;
  // Start another operation, then reset it at the next epoch boundary.
  #5;clk=1;#5;clk=0;
 end
 $display("PASS color miter epochs=3 groups=6144 pixels=%0d CE0_and_inflight_reset=1 invalid_packet_fault=3",total);$finish;
end
initial begin #600000;$fatal(1,"watchdog");end
endmodule
