// Two logical DMA jobs, independent request/return cursors, no payload storage.
module gpu_command_dma(
 input clk,reset,ce,stop,
 input cache_valid,output cache_ready,input[21:0]cache_source,input cache_bank,
 output reg cache_done,output reg cache_done_bank,
 input work_valid,output work_ready,input work_mesh,input[21:0]work_source,
 input[6:0]work_destination,input[1:0]work_size,input[2:0]work_blocks,input work_slot,
 output work_busy,output reg work_done,output reg mesh_begin,mesh_done,output mesh_slot,
 output reg uniform_done,output reg[1:0]uniform_bank,
 output dma_we,output[8:0]dma_addr,output[63:0]dma_data,
 output request_valid,input request_ready,output[21:0]request_address,output[1:0]request_count,
 input response_valid,response_last,response_error,response_bad,input[63:0]response_data,
 output idle,mesh_loading,output reg mesh_begin_slot,mesh_done_slot,output reg fault
);

 // Two logical jobs; issuing their last request does not retire their storage.
 reg[17:0]js[0:1];reg[6:0]jd[0:1];reg[1:0]jz[0:1];reg[3:0]jl[0:1];
 reg jm[0:1],jpage[0:1],jh[0:1];reg[1:0]put=0,issued=0,retired=0;
 wire[1:0]jobs=put-retired;
 reg offer=0,offer_cache=0,offer_last=0;reg[17:0]oa;reg[8:0]od;reg[1:0]oc;
 // Only return metadata is queued. Every 64-bit word writes its final RAM row.
 reg[6:0]next_destination;reg rk[0:1],rl[0:1];reg[1:0]rp=0,rh=0;reg[8:0]cursor=0;
 wire[1:0]returns=rp-rh;
 localparam IDLE=0,REQUEST=1,RESPONSE=2;
 wire[1:0]state=returns!=0?RESPONSE:offer?REQUEST:IDLE;
 wire terminal=response_valid&&(response_last||response_error);
 wire take=offer&&request_ready;
 wire pop=returns!=0&&terminal;
 wire build=!offer&&returns<2&&!stop&&!fault;
 wire accept_work=ce&&work_valid&&work_ready;
 assign work_ready=jobs<2&&!stop&&!fault;
 assign work_busy=jobs!=0;
 assign mesh_slot=jpage[retired[0]];
 assign mesh_loading=jobs!=0&&jm[retired[0]]&&
  (jobs==1||(jm[!retired[0]]&&jpage[!retired[0]]==jpage[retired[0]]));
 assign cache_ready=build&&cache_valid;
 assign request_valid=offer&&!reset;assign request_address={oa,4'd0};assign request_count=oc;
 assign dma_we=returns!=0&&response_valid&&!response_error&&!response_bad&&!fault&&!stop;
 assign dma_addr=cursor;assign dma_data=response_data;
 assign idle=!offer&&returns==0&&jobs==0;
 always @(posedge clk)begin
  if(reset)begin
   put<=0;issued<=0;retired<=0;rp<=0;rh<=0;offer<=0;cursor<=0;fault<=0;
   work_done<=0;mesh_begin<=0;mesh_done<=0;mesh_begin_slot<=0;mesh_done_slot<=0;
   uniform_done<=0;uniform_bank<=0;cache_done<=0;cache_done_bank<=0;
   js[0]<=0;js[1]<=0;jd[0]<=0;jd[1]<=0;jm[0]<=0;jm[1]<=0;jpage[0]<=0;jpage[1]<=0;
  end else begin
   work_done<=0;mesh_begin<=0;mesh_done<=0;uniform_done<=0;cache_done<=0;
   if(accept_work)begin
    js[put[0]]<=work_source[21:4];jd[put[0]]<=work_mesh?{6'b100000,work_slot}:work_destination;
    jm[put[0]]<=work_mesh;jpage[put[0]]<=work_slot;jh[put[0]]<=work_mesh;
    jz[put[0]]<=work_size;jl[put[0]]<={1'b0,work_blocks}+1'b1;put<=put+1'b1;
    if(work_mesh)begin mesh_begin<=1;mesh_begin_slot<=work_slot;end
   end
   if(build)begin
    if(cache_valid)begin
     offer<=1;offer_cache<=1;offer_last<=0;oa<=cache_source[21:4];oc<=3;
     od<={4'b1110,cache_bank,4'd0};
    end else if(issued!=put)begin
     offer<=1;offer_cache<=0;oa<=js[issued[0]];od<={jd[issued[0]],2'd0};
     oc<=jm[issued[0]]?(jh[issued[0]]?2'd0:2'd3):{jz[issued[0]][1],|jz[issued[0]]};
     offer_last<=!jm[issued[0]]||(!jh[issued[0]]&&jl[issued[0]]==1);
    end
   end
   if(take)begin
    offer<=0;if(returns!=0)next_destination<=od[8:2];rk[rp[0]]<=offer_cache;rl[rp[0]]<=offer_last;rp<=rp+1'b1;
    if(!offer_cache)begin
     if(jm[issued[0]])begin
      js[issued[0]]<=js[issued[0]]+18'd4;
      if(jh[issued[0]])begin jh[issued[0]]<=0;jd[issued[0]]<={1'b0,jpage[issued[0]],5'd0};end
      else begin jd[issued[0]]<=jd[issued[0]]+7'd4;jl[issued[0]]<=jl[issued[0]]-1'b1;end
     end
     if(offer_last)issued<=issued+1'b1;
    end
   end
   if(dma_we)cursor<=cursor+1'b1;
   if(take&&returns==0)cursor<=od;
   if(response_valid&&returns!=0)begin
    if(response_bad||response_error)fault<=1;
    if(terminal)begin
     rh<=rh+1'b1;
     if(returns>1)cursor<={next_destination,2'd0};else if(take)cursor<=od;
     if(!stop&&!fault&&!response_bad&&!response_error)begin
      if(rk[rh[0]])begin cache_done<=1;cache_done_bank<=cursor[4];end
      else if(rl[rh[0]])begin
       work_done<=1;retired<=retired+1'b1;
       if(jm[retired[0]])begin mesh_done<=1;mesh_done_slot<=jpage[retired[0]];end
       else if(jd[retired[0]][6:4]==3'b101)begin uniform_done<=1;uniform_bank<=jd[retired[0]][3:2];end
      end
     end
    end
   end
   if((stop||fault)&&!offer&&returns==0)begin retired<=put;issued<=put;end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset)begin
  if(take&&od[1:0]!=0)$fatal(1,"DMA destination alignment proof");
  if(jobs>2||returns>2)$fatal(1,"DMA queue capacity");
  if(accept_work&&((|work_source[3:0])||(!work_mesh&&
   ((work_size==1&&work_source[4])||(work_size==2&&(|work_source[5:4]))||work_size==3))))
   $fatal(1,"CP copy source is not size aligned");
  if(cache_valid&&cache_ready&&(|cache_source[5:0]))$fatal(1,"CP command page is not 128-byte aligned");
 end
`endif
endmodule
