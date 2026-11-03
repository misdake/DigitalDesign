`timescale 1ns/1ps
module tb;
localparam PRECODE_BITS=9;
reg pixel_clock=0,pixel_reset=0;
reg active=0,hs=0,vs=0;
reg [7:0] r=0,g=0,b=0;
reg [8:0] next_r=256,next_g=256,next_b=256;
reg visible_pipe3=0,hsync_pipe3=0,vsync_pipe3=0;
reg [26:0] precode_pipe={3{9'h100}};
// Trigger the reference combinational block after elaboration, before any clock.
reg [23:0] rgb_ref;
initial begin #0;rgb_ref=0;end
wire [8:0] red=precode_pipe[26:18],green=precode_pipe[17:9],blue=precode_pipe[8:0];
wire [9:0] blue_symbol,green_symbol,red_symbol;
__LANES__
// Controlled producer with the same registered black/DE/sync boundary as C2.
always @(posedge pixel_clock)begin
 if(pixel_reset)begin precode_pipe<={3{9'h100}};visible_pipe3<=0;hsync_pipe3<=0;vsync_pipe3<=0;rgb_ref<=0;end
 else begin
  precode_pipe<=active?{next_r,next_g,next_b}:{3{9'h100}};
  rgb_ref<=active?{r,g,b}:24'd0;
  visible_pipe3<=active;hsync_pipe3<=hs;vsync_pipe3<=vs;
 end
end
wire [9:0] rr,rg,rb;
DisplayTmdsReference ref_r(.clk(pixel_clock),.reset(pixel_reset),.de(visible_pipe3),.control(2'b00),.data(rgb_ref[23:16]),.symbol(rr));
DisplayTmdsReference ref_g(.clk(pixel_clock),.reset(pixel_reset),.de(visible_pipe3),.control(2'b00),.data(rgb_ref[15:8]),.symbol(rg));
DisplayTmdsReference ref_b(.clk(pixel_clock),.reset(pixel_reset),.de(visible_pipe3),.control({vsync_pipe3,hsync_pipe3}),.data(rgb_ref[7:0]),.symbol(rb));
reg [2047:0] vectors;
integer fd,result,index,pe,rst,de,h,v,count=0,j;
integer cov[0:2][0:8];reg[255:0] seen[0:2];
task coverage;
 input integer lane;input signed[5:0] d;input[8:0] qm;input[7:0] data;
 integer n;
 begin
 n=qm[0]+qm[1]+qm[2]+qm[3]+qm[4]+qm[5]+qm[6]+qm[7];
 cov[lane][0]=cov[lane][0]+(d<0);cov[lane][1]=cov[lane][1]+(d==0);cov[lane][2]=cov[lane][2]+(d>0);
 cov[lane][3]=cov[lane][3]+(n==4);cov[lane][4]=cov[lane][4]+(!qm[8]);cov[lane][5]=cov[lane][5]+qm[8];
 if(d==0||n==4)cov[lane][6]=cov[lane][6]+1;
 else if((d>0&&n>4)||(d<0&&n<4))cov[lane][7]=cov[lane][7]+1;
 else cov[lane][8]=cov[lane][8]+1;
 seen[lane][data]=1;
 end
endtask
initial begin
 for(j=0;j<3;j=j+1)begin seen[j]=0;for(v=0;v<9;v=v+1)cov[j][v]=0;end
 if(!$value$plusargs("vectors=%s",vectors))$fatal(1,"vectors");fd=$fopen(vectors,"r");if(!fd)$fatal(1,"open");
 while(!$feof(fd))begin
 result=$fscanf(fd,"%d %d %d %d %d %d %h %h %h %h %h %h\n",index,pe,rst,de,h,v,r,g,b,next_r,next_g,next_b);
 if(result!=12)$fatal(1,"vector fields %0d",result);
 pixel_reset=rst;active=de;hs=h;vs=v;#1;
 if(index==0&&{red_symbol,green_symbol,blue_symbol}!=={30{1'bx}})$fatal(1,"TMDS_INITIAL symbol declaration");
 $write("R %0d %h %h %h %h %h %h ",index,red_symbol,green_symbol,blue_symbol,u_red.disparity,u_green.disparity,u_blue.disparity);
 $write("%h %h %h %h %h %h ",u_red.qm,u_green.qm,u_blue.qm,rgb_ref[23:16],rgb_ref[15:8],rgb_ref[7:0]);
 if(pe&&!pixel_reset&&visible_pipe3)begin
 coverage(0,u_red.disparity,u_red.qm,rgb_ref[23:16]);coverage(1,u_green.disparity,u_green.qm,rgb_ref[15:8]);coverage(2,u_blue.disparity,u_blue.qm,rgb_ref[7:0]);end
 pixel_clock=pe;#1;
 $display("%h %h %h %h %h %h",red_symbol,green_symbol,blue_symbol,u_red.disparity,u_green.disparity,u_blue.disparity);
 if({red_symbol,green_symbol,blue_symbol}!=={rr,rg,rb} ||
 {u_red.disparity,u_green.disparity,u_blue.disparity}!=={ref_r.disparity,ref_g.disparity,ref_b.disparity})$fatal(1,"TMDS_MITER row=%0d actual=%h/%h/%h ref=%h/%h/%h d=%h/%h/%h rd=%h/%h/%h",index,red_symbol,green_symbol,blue_symbol,rr,rg,rb,u_red.disparity,u_green.disparity,u_blue.disparity,ref_r.disparity,ref_g.disparity,ref_b.disparity);
 pixel_clock=0;#1;count=count+1;if(count>32768)$fatal(1,"row cap");
 end
 for(j=0;j<3;j=j+1)begin
 for(v=0;v<9;v=v+1)if(cov[j][v]==0)$fatal(1,"missing coverage lane%0d field%0d",j,v);
 if(seen[j]!=={256{1'b1}})$fatal(1,"missing byte lane%0d",j);
 $display("COV %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d",j,cov[j][0],cov[j][1],cov[j][2],cov[j][3],cov[j][4],cov[j][5],cov[j][6],cov[j][7],cov[j][8]);end
 $fclose(fd);$display("PASS TMDS %0d",count);$finish;
end
initial begin #200000;$fatal(1,"time cap");end
endmodule
