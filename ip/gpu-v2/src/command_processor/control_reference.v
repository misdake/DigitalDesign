// Semantic control pins only; not an encoded command ABI. External payload
// owner retains validated descriptor/context/lease data until command_done.
module gpu_v2_command_control(
 input wire clk,reset,ce,command_valid,
 input wire command_dma,command_wait,command_draw,command_release,command_fence,command_unsupported,
 input wire[2:0]command_token,input wire command_validated,
 input wire[7:0]event_set,event_ack,input wire[3:0]dma_done,
 input wire dma_ready,draw_ready,draw_done,release_ready,fence_ready,external_fault,
 output wire command_capture,command_done,handler_valid,output reg[2:0]handler_id,
 output wire dma_valid,dma_accept,output wire[1:0]dma_token,
 output wire wait_ack,draw_valid,draw_accept,release_valid,release_accept,fence_ack,fault,
 output wire[2:0]phase_state,token_state,output wire[3:0]tokens_state,reserved_state,
 output wire[7:0]pending_state,output wire[2:0]cursor_state,output wire draw_return_state
);
 localparam IDLE=3'd0,DMA=3'd1,WAIT=3'd2,DRAW_REQ=3'd3,DRAW_RUN=3'd4,RELEASE=3'd5,FENCE=3'd6,FAULT=3'd7;
 reg[2:0]phase;reg[1:0]token;reg[3:0]tokens,reserved;reg[7:0]pending;reg[2:0]cursor;reg draw_return;
 wire[4:0]kinds={command_fence,command_release,command_draw,command_wait,command_dma};
 wire invalid_kind=command_unsupported||kinds==0||((kinds&(kinds-5'd1))!=0);
 wire invalid_token=(command_dma||command_wait)&&(command_token[2]||(command_dma&&reserved[command_token[1:0]]));
 wire bad=external_fault||(dma_done&~reserved)!=0||(draw_done&&phase!=DRAW_RUN)
  ||(ce&&phase==IDLE&&command_valid&&(!command_validated||invalid_kind||invalid_token));
 assign fault=!reset&&(phase==FAULT||bad);
 wire enabled=ce&&!reset&&!fault;
 wire[3:0]accepted_done=dma_done&reserved;
 wire[7:0]sets=event_set|{4'b0,accepted_done}|((phase==IDLE&&command_valid)?8'h10:8'h00)|(fault?8'h80:8'h00);
 wire[7:0]available=pending|sets;
 reg chosen;integer scan;reg[2:0]candidate;
 always @*begin
  chosen=0;handler_id=0;candidate=0;
  for(scan=0;scan<8;scan=scan+1)begin
   candidate=cursor+scan;
   if(!chosen&&candidate!=5&&candidate!=6&&available[candidate])begin chosen=1;handler_id=candidate;end
  end
 end
 assign handler_valid=!reset&&ce&&(phase==IDLE||phase==WAIT||phase==FAULT)&&chosen;
 assign command_capture=enabled&&phase==IDLE&&handler_valid&&handler_id==4&&command_valid;
 assign dma_valid=enabled&&phase==DMA;assign dma_accept=dma_valid&&dma_ready;assign dma_token=reset?2'b0:token;
 wire[3:0]completed_tokens=tokens|accepted_done;
 assign wait_ack=enabled&&phase==WAIT&&completed_tokens[token];
 assign draw_valid=enabled&&phase==DRAW_REQ;assign draw_accept=draw_valid&&draw_ready;
 assign release_valid=enabled&&phase==RELEASE;assign release_accept=release_valid&&release_ready;
 assign fence_ack=enabled&&phase==FENCE&&fence_ready;
 assign command_done=dma_accept||wait_ack||release_accept||fence_ack||(enabled&&phase==DRAW_RUN&&draw_return);
 wire[7:0]acks=(ce?event_ack:8'd0)|(command_done?8'h10:8'h00)|(wait_ack?(8'd1<<token):8'd0);
 assign phase_state=phase;assign token_state={1'b0,token};assign tokens_state=tokens;assign reserved_state=reserved;
 assign pending_state=pending;assign cursor_state=cursor;assign draw_return_state=draw_return;
 always @(posedge clk)begin
  if(reset)begin phase<=IDLE;token<=0;tokens<=0;reserved<=0;pending<=0;cursor<=0;draw_return<=0;end
  else begin
   pending<=(pending&~acks)|sets;
   if(handler_valid)cursor<=handler_id+3'd1;
   tokens<=(tokens|accepted_done)&~(wait_ack?(4'b1<<token):4'd0);
   reserved<=(reserved|(dma_accept?(4'b1<<token):4'd0))&~(wait_ack?(4'b1<<token):4'd0);
   if(phase==DRAW_RUN&&draw_done)draw_return<=1;
   if(fault)phase<=FAULT;
   else if(command_capture)begin
    token<=command_token[1:0];
    if(command_dma)phase<=DMA;
    else if(command_wait)phase<=WAIT;
    else if(command_draw)phase<=DRAW_REQ;
    else if(command_release)phase<=RELEASE;
    else phase<=FENCE;
   end else if(command_done)begin phase<=IDLE;draw_return<=0;end
   else if(draw_accept)begin phase<=DRAW_RUN;draw_return<=0;end
  end
 end
endmodule
