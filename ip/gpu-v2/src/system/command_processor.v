// Sequential 32-bit CP; input RAM Q is consumed before yielding to V/T.
module gpu_command_processor(
 input clk,reset,ce,abort,load_count,submit,input[15:0]control_data,
 output submit_ready,output reg busy,output stop,
 input render_idle,render_fault,memory_busy,meshlet_ready,meshlet_done,
 output start,output draw_slot,
 output cp_re,output[8:0]cp_addr,input[63:0]cp_q,input cp_ready,
 output cache_valid,input cache_ready,output[21:0]cache_source,output cache_bank,
 input cache_done,cache_done_bank,
 output work_valid,input work_ready,output work_mesh,output[21:0]work_source,
 output[6:0]work_destination,output[1:0]work_size,output[2:0]work_blocks,output work_slot,
 input work_busy,dma_idle,dma_fault,mesh_loading,mesh_begin,mesh_done,mesh_slot,mesh_begin_slot,mesh_done_slot,
 input uniform_done,input[1:0]uniform_bank,
 input header_we,header_slot,input[63:0]header_data,
 output reg[31:0]cfg_far_z,output reg[15:0]texture_base,output reg[3:0]texture_n,
 output reg texture_mip,output reg[1:0]texture_filter,
 output reg signed[7:0]cfg_lod_bias,output reg draw_unlit,
 output reg[13:0]color_base,depth_base,output reg[31:0]mesh_color,
 output reg[23:0]specular,
 output reg[2:0]depth_func,output reg depth_write,blend,output texture_invalidate,
 output reg[15:0]context_epoch,output reg[1:0]context_mode,output reg[4:0]context_code,
 output reg signed[15:0]light_x,light_y,light_z,ray_x,ray_y,ray_k,
 output reg[8:0]ambient,directional,output flush_request,input flush_complete,
 output clear_active,clear_request,output[31:0]clear_address,output[15:0]clear_value,
 input clear_ready,clear_complete,clear_ok,
 output ui_start,input ui_done,output[9:0]ui_first,ui_last
);
 localparam NOP=0,DMA_LOCAL=1,DMA_MESHLET=2,DRAW=3,WAIT=4,SET_GLOBAL=5,SET_RENDER=6,FLUSH=7,CLEAR=8,TEX_INVALIDATE=9,SET_TARGET=10,UI_GRID=11,END=15;
 localparam IDLE=0,FETCH=1,EXEC=2,DRAW_WAIT=3,APPLY_R=4,APPLY_W=5,
  WAITING=7,FLUSHING=8,ENDING=9,FAILED=10,LATCH=11,CLEAR_REQUEST=12,CLEAR_WAIT=13,CLEAR_DONE=14,
  COLOR_R=15,COLOR_W=16,DRAW_START=17,UI_WAIT=18;
 reg[4:0]state;
 reg failed,all_fence,global_valid,render_valid,stored,target_valid;
 reg[5:0]pc;reg[15:0]fetch_line;reg[9:0]fetch_left;
`ifndef SYNTHESIS
 reg[16:0]diagnostic_pc,limit;reg count_seen;
`endif
 reg fetch_bank,cache_inflight;reg[1:0]cache_pages,page_valid;
 reg[3:0]uniform_valid,page_uniform;
 reg[1:0]object_index;
 reg[31:0]command;
 reg[1:0]clear_flight;reg clear_offer;
 wire[31:0]read_word=(state==LATCH&&pc[0])?cp_q[63:32]:cp_q[31:0];
 wire[3:0]opcode=command[31:28];
 wire[1:0]wait_mask=command[1:0];
 wire apply_render=opcode==SET_RENDER;
 wire[6:0]destination=command[9:3];wire[1:0]size=command[2:1];
 // Check only the low address window that can cross the physical limit.
 // Size codes describe one, two or four 32-byte blocks.
 wire local_source_overflow=(&command[27:12])&&
  ((size==1&&(&command[11:10]))||(size==2&&(|command[11:10])));
 wire staging_target=destination[6:4]==3'b100&&destination[3:0]>=2&&
  !(size==1&&(&destination[3:0]))&&
  !(size==2&&destination[3:2]==3&&(|destination[1:0]));
 wire uniform_target=destination[6:4]==3'b101&&destination[1:0]==0&&size==2;
 wire local_aligned=(size==0)||(size==1&&!command[10])||(size==2&&command[11:10]==0);
 wire local_legal=!command[0]&&size!=3&&local_aligned&&!local_source_overflow&&(staging_target||uniform_target);
 wire[4:0]mesh_tail={1'b0,command[15:12]}+{2'b0,command[3:1]}+5'd1;
 wire mesh_legal=command[11:4]==0&&(!(&command[27:16])||!mesh_tail[4]);
 wire object_legal=command[27:7]==0&&command[6:4]==3'b100&&command[3:0]>=2&&
  !(opcode==SET_GLOBAL&&(&command[3:0]));
 assign stop=abort||failed;
 assign submit_ready=!busy&&!failed&&!abort;
 assign cache_valid=busy&&!stop&&state!=ENDING&&state!=UI_WAIT&&
  !(opcode==UI_GRID&&(state==EXEC||state==APPLY_R||state==APPLY_W))&&fetch_left!=0&&!cache_inflight&&!cache_pages[fetch_bank];
 assign cache_source={fetch_line,6'd0};assign cache_bank=fetch_bank;
 assign cp_re=!stop&&((state==FETCH&&cache_pages[pc[5]])||state==APPLY_R||state==COLOR_R);
 assign cp_addr=state==COLOR_R?{3'b101,page_uniform[{command[0],1'b0}+:2],4'b1011}:
  state==APPLY_R?{command[6:0],object_index}:{4'b1110,pc[5],pc[4:1]};
 assign work_valid=state==EXEC&&!stop&&(opcode==DMA_LOCAL||opcode==DMA_MESHLET);
 assign work_mesh=opcode==DMA_MESHLET;
 assign work_source=work_mesh?{command[27:12],6'd0}:{command[27:10],4'd0};
 assign work_destination=destination;assign work_size=size;assign work_blocks=command[3:1];assign work_slot=command[0];
 wire draw_bindings=global_valid&&render_valid&&uniform_valid[page_uniform[{command[0],1'b0}+:2]];
 wire draw_inputs=page_valid[command[0]]&&(!work_busy||(mesh_loading&&mesh_slot!=command[0]));
 assign start=state==DRAW_START&&draw_inputs&&!stop;
 assign draw_slot=command[0];
 assign texture_invalidate=ce&&!stop&&state==EXEC&&opcode==TEX_INVALIDATE;
 assign flush_request=state==FLUSHING&&!stop;
 // CLEAR names the current plane and carries its repeated value directly.
 // Reuse reserved instruction bits as the sole 0..1499 sector cursor.
 assign clear_active=state==CLEAR_REQUEST||state==CLEAR_WAIT;
 assign clear_request=clear_offer;
 wire[10:0]clear_sector=command[27:17];
 wire[6:0]clear_page=(command[16]?depth_base[13:7]:color_base[13:7])+{5'd0,clear_sector[10:9]};
 assign clear_address={9'd0,clear_page,clear_sector[8:0],7'd0};
 assign clear_value=command[15:0];
 assign ui_start=ce&&!stop&&state==APPLY_W&&opcode==UI_GRID;
 // The instruction remains resident throughout UI_WAIT; no extra range FFs.
 assign ui_first=command[27]?command[16:7]:10'd0;
 assign ui_last=command[27]?command[26:17]:10'd749;
 wire waits_done=(!wait_mask[0]||!work_busy)&&(!wait_mask[1]||render_idle);
 reg object_bad;
 always @*begin
  object_bad=0;
  if(opcode==SET_TARGET)object_bad=cp_q[63:28]!=0||(|cp_q[6:0])||(|cp_q[20:14])||cp_q[13:0]>16009||cp_q[27:14]>16009||
   !(({1'b0,cp_q[13:0]}+15'd375<={1'b0,cp_q[27:14]})||({1'b0,cp_q[27:14]}+15'd375<={1'b0,cp_q[13:0]}));
  else if(apply_render)case(object_index)
   0:object_bad=cp_q[63:56]!=0; // Texture fields are trusted driver data.
   1:object_bad=cp_q[31:6]!=0||cp_q[63:40]!=0;
   default:object_bad=cp_q!=0;
  endcase else case(object_index)
   0:object_bad=cp_q[31:0]==0||cp_q[31:0]>=200*65536;
   1:begin end
   2:object_bad=(cp_q[31:0]&32'hfe00fe00)!=0||cp_q[8:0]>256||cp_q[24:16]>256||cp_q[47:39]!=0||cp_q[38:34]>16;
   default:object_bad=cp_q!=0;
  endcase
 end
 task fail;begin
  failed<=1;if(!clear_active)state<=FAILED;fetch_left<=0;
 end endtask
 task advance;begin
  if(pc[4:0]==31)cache_pages[pc[5]]<=0;
  pc<=pc+1'b1;state<=FETCH;
`ifndef SYNTHESIS
  diagnostic_pc<=diagnostic_pc+17'd4;
`endif
 end endtask
 always @(posedge clk)begin
  if(reset)begin
   state<=IDLE;command<=0;clear_flight<=0;clear_offer<=0;failed<=0;busy<=0;
   all_fence<=0;global_valid<=0;render_valid<=0;stored<=0;target_valid<=0;pc<=0;
`ifndef SYNTHESIS
   diagnostic_pc<=0;limit<=0;count_seen<=0;
`endif
   color_base<=0;depth_base<=0;mesh_color<=32'hffffffff;
   fetch_line<=0;fetch_left<=0;fetch_bank<=0;cache_inflight<=0;cache_pages<=0;page_valid<=0;
   uniform_valid<=0;page_uniform<=0;object_index<=0;
   cfg_far_z<=0;texture_base<=0;texture_n<=0;texture_mip<=0;texture_filter<=0;cfg_lod_bias<=0;
   draw_unlit<=0;specular<=0;depth_func<=0;depth_write<=0;blend<=0;
   context_epoch<=0;context_mode<=0;context_code<=0;light_x<=0;light_y<=0;light_z<=0;
   ray_x<=0;ray_y<=0;ray_k<=0;ambient<=0;directional<=0;
  end else begin
   // Device writes are wall-clock events: a one-cycle SEND survives CE0.
   if(load_count&&submit_ready)begin
    fetch_left<={1'b0,control_data[13:5]}+10'd1;
`ifndef SYNTHESIS
    limit<={1'b0,control_data[13:0],2'b00}+17'd4;count_seen<=1;
`endif
   end
   if(cache_valid&&cache_ready)begin cache_inflight<=1;fetch_line<=fetch_line+1'b1;fetch_left<=fetch_left-1'b1;fetch_bank<=!fetch_bank;end
   if(cache_done)begin cache_inflight<=0;cache_pages[cache_done_bank]<=1;end
   if(mesh_begin)page_valid[mesh_begin_slot]<=0;
   if(mesh_done)page_valid[mesh_done_slot]<=1;
   if(uniform_done)uniform_valid[uniform_bank]<=1;
   if(header_we)page_uniform[{header_slot,1'b0}+:2]<=header_data[17:16];
   if(busy&&state==ENDING&&render_idle&&dma_idle&&!memory_busy)begin busy<=0;state<=IDLE;end
   if(submit&&submit_ready)begin
     busy<=1;pc<=0;
`ifndef SYNTHESIS
     diagnostic_pc<=0;count_seen<=0;
`endif
     fetch_line<=control_data;
     fetch_bank<=0;cache_inflight<=0;cache_pages<=0;page_valid<=0;uniform_valid<=0;
     all_fence<=0;global_valid<=0;render_valid<=0;stored<=0;target_valid<=0;
     state<=FETCH;
    end
   else if(ce&&!stop)case(state)
    FETCH:if(cache_pages[pc[5]]&&cp_ready)state<=LATCH;
    LATCH:begin command<=read_word;state<=EXEC;end
    EXEC:case(opcode)
     NOP:advance();
     DMA_LOCAL,DMA_MESHLET:if(work_ready)advance();
     DRAW:if(draw_inputs)state<=COLOR_R;
     WAIT:state<=WAITING;
     SET_GLOBAL,SET_RENDER,SET_TARGET:begin object_index<=0;state<=APPLY_R;end
     UI_GRID:if(dma_idle)begin object_index<=0;state<=APPLY_R;end
     CLEAR:begin clear_flight<=0;clear_offer<=0;state<=CLEAR_REQUEST;end
     FLUSH:state<=FLUSHING;
     TEX_INVALIDATE:advance();
     END:begin fetch_left<=0;state<=ENDING;end
     default:advance();
    endcase
    COLOR_R:if(cp_ready)state<=COLOR_W;
    COLOR_W:begin mesh_color<=cp_q[31:0];state<=DRAW_START;end
    DRAW_START:if(start&&meshlet_ready)begin all_fence<=0;stored<=0;state<=DRAW_WAIT;end
    DRAW_WAIT:if(meshlet_done)advance();
    WAITING:if(waits_done)begin if(wait_mask[1])all_fence<=1;advance();end
    APPLY_R:if(cp_ready)state<=APPLY_W;
    APPLY_W:if(opcode==UI_GRID)begin page_valid<=0;state<=UI_WAIT;end
    else if(opcode==SET_TARGET)begin
     color_base<={cp_q[13:7],7'd0};depth_base<={cp_q[27:21],7'd0};target_valid<=1;stored<=0;advance();
    end else begin
     if(apply_render)case(object_index)
      0:begin texture_base<=cp_q[15:0];texture_n<=cp_q[19:16];texture_mip<=cp_q[20];texture_filter<=cp_q[22:21];
       specular<=cp_q[55:32];end
      1:begin depth_func<=cp_q[2:0];depth_write<=cp_q[3];blend<=cp_q[4];draw_unlit<=cp_q[5];
       cfg_lod_bias<=cp_q[39:32];end
      default:begin end
     endcase else case(object_index)
      0:begin cfg_far_z<=cp_q[31:0];light_x<=cp_q[47:32];light_y<=cp_q[63:48];end
      1:begin light_z<=cp_q[15:0];ray_x<=cp_q[31:16];ray_y<=cp_q[47:32];ray_k<=cp_q[63:48];end
      2:begin ambient<=cp_q[8:0];directional<=cp_q[24:16];context_mode<=cp_q[33:32];context_code<=cp_q[38:34];context_epoch<=cp_q[63:48];end
      default:begin end
     endcase
     if(object_index==(apply_render?2'd1:2'd2))begin
      if(apply_render)render_valid<=1;else global_valid<=1;advance();
     end else begin object_index<=object_index+1'b1;state<=APPLY_R;end
    end
    FLUSHING:if(flush_complete)begin stored<=1;advance();end
    CLEAR_DONE:advance();
    UI_WAIT:if(ui_done)advance();
    default:begin end
   endcase
   // Accepted physical work follows wall time, including CE0 and failure.
   // A presented request is never withdrawn by abort; stop after its terminal.
   if(state==CLEAR_REQUEST&&!clear_offer&&clear_flight<2&&!stop)clear_offer<=1;
   if(clear_offer&&clear_ready)begin
    clear_offer<=0;
    if(clear_sector==1499||stop)state<=CLEAR_WAIT;
    else command[27:17]<=clear_sector+1'b1;
   end
   case({clear_offer&&clear_ready,clear_active&&clear_complete})
    2'b10:clear_flight<=clear_flight+1'b1;
    2'b01:clear_flight<=clear_flight-1'b1;
   endcase
   if(clear_active&&stop&&!clear_offer)state<=CLEAR_WAIT;
   if(state==CLEAR_WAIT&&!clear_offer&&(clear_flight==0||(clear_flight==1&&clear_complete)))state<=stop?FAILED:CLEAR_DONE;
   if(busy&&abort)fail();
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset)begin
  if(load_count&&(!submit_ready||control_data[15:14]!=0))$fatal(1,"CP count SEND contract");
  if(submit&&(!submit_ready||!count_seen||({1'b0,control_data}+fetch_left)>17'd65536))$fatal(1,"CP base SEND contract");
  if(ce&&!stop&&state==FETCH&&diagnostic_pc>=limit)$fatal(1,"CP missing END");
  if(ce&&!stop&&state==EXEC)case(opcode)
   NOP:if(command[27:0]!=0)$fatal(1,"CP NOP encoding");
   DMA_LOCAL:if(!local_legal)$fatal(1,"CP local DMA contract");
   DMA_MESHLET:if(!mesh_legal)$fatal(1,"CP mesh DMA contract");
   DRAW:if(command[27:1]!=0||!draw_bindings||!target_valid)$fatal(1,"CP DRAW contract");
   WAIT:if(command[27:2]!=0)$fatal(1,"CP WAIT encoding");
   SET_GLOBAL,SET_RENDER:if(!object_legal||!all_fence||!render_idle||work_busy)$fatal(1,"CP state command contract");
   CLEAR:if(command[27:17]!=0||!target_valid||!all_fence||!render_idle||work_busy)$fatal(1,"CP plane CLEAR contract");
   SET_TARGET:if(!object_legal||!all_fence||!stored||!render_idle||work_busy)$fatal(1,"CP target drain contract");
   UI_GRID:if(command[6:4]!=3'b100||command[3:0]<2||ui_first>ui_last||ui_last>=750||
    (!command[27]&&command[26:7]!=0)||!all_fence||!stored||!render_idle||work_busy||!target_valid)$fatal(1,"CP UI drain contract");
   FLUSH,TEX_INVALIDATE:if(command[27:0]!=0||!all_fence||!render_idle||work_busy)$fatal(1,"CP drain contract");
   END:if(command[27:0]!=0||diagnostic_pc+17'd4!=limit||!stored||!all_fence||work_busy)$fatal(1,"CP END contract");
   default:$fatal(1,"CP opcode contract");
  endcase
  if(ce&&!stop&&state==APPLY_W&&opcode!=UI_GRID&&object_bad)$fatal(1,"CP state data contract");
  if(ui_start&&cp_q[63:23]!=0)$fatal(1,"CP UI descriptor");
  if(ce&&!stop&&state==COLOR_W&&cp_q[63:32]!=0)$fatal(1,"CP mesh color encoding");
  if(clear_active&&clear_complete&&!clear_ok)$fatal(1,"CP CLEAR memory response");
  if(busy&&!abort&&!failed&&(render_fault||dma_fault))$fatal(1,"CP downstream contract");
  if(ce&&start&&meshlet_ready&&work_busy&&(!mesh_loading||mesh_slot==draw_slot))$fatal(1,"DRAW overlaps DMA writer");
 end
`endif
endmodule
