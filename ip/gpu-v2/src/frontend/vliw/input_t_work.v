// 按T的最后消费排程分配十个物理槽。原ROM逻辑地址/算术完全保持。
// 两颗原输入BSRAM只接DMA，私有16x36 SSRAM同时提供T的一写一读。
module rs_static_scratchpad #(parameter COMPACT=1)(input clk,reset,dma_we,input[9:0]dma_addr,
 input[63:0]dma_data,input le,re,work_we,input[9:0]la,ra,work_wa,input[35:0]work_data,
 output[35:0]lq,rq);
 function[3:0]slot;
  input[5:0]a;
  begin
   case(a)
    16:slot=6;17:slot=9;18:slot=7;
    19:slot=2;20:slot=0;21:slot=8;
    22:slot=1;23:slot=5;24:slot=4;
    default:slot=a[3:0];
   endcase
  end
 endfunction
 wire[35:0]dl={{4{dma_data[31]}},dma_data[31:0]};
 wire[35:0]dr={{4{dma_data[63]}},dma_data[63:32]};
 wire work_read=re&&ra[9:6]==4'b0110;
 wire source_read=re&&ra[9:6]!=4'b0110;
 wire[35:0]source_q,work_q;
 reg read_work;reg[3:0]work_address;
 iz_ram36 left_input(clk,reset,1'b1,dma_we,dma_addr[8:0],dl,le,la[8:0],lq);
 iz_ram36 right_input(clk,reset,1'b1,dma_we,dma_addr[8:0],dr,source_read,ra[8:0],source_q);
 iz_params_async #(.WIDTH(36)) triangle_work(clk,reset,1'b1,work_we,slot(work_wa[5:0]),work_data,work_address,work_q);
 always @(posedge clk)begin
  if(reset)begin read_work<=0;work_address<=0;end
  else begin
   if(re)read_work<=work_read;
   if(work_read)work_address<=slot(ra[5:0]);
  end
 end
 assign rq=read_work ? work_q : source_q;
`ifndef SYNTHESIS
 reg[5:0]logical_owner[0:15];reg[15:0]known;
 integer i;
 initial if(!COMPACT)$fatal(1,"静态输入分库只支持compact地址契约");
 always @(posedge clk)begin
  if(reset)begin known<=0;for(i=0;i<16;i=i+1)logical_owner[i]<=0;end
  else begin
   if(work_read)begin
    if(!known[slot(ra[5:0])]||logical_owner[slot(ra[5:0])]!=ra[5:0])$fatal(1,"T静态槽在最后读取前被覆盖");
    if(work_we&&slot(work_wa[5:0])==slot(ra[5:0]))$fatal(1,"T静态槽同沿读写 read%0d write%0d slot%0d",ra[5:0],work_wa[5:0],slot(ra[5:0]));
   end
   if(work_we)begin known[slot(work_wa[5:0])]<=1;logical_owner[slot(work_wa[5:0])]<=work_wa[5:0];end
   if(work_we&&work_wa[9:6]!=4'b0110)$fatal(1,"T写超出私有工作页");
   if(dma_we&&dma_addr[9])$fatal(1,"DMA超出compact输入页");
  end
 end
`endif
endmodule
