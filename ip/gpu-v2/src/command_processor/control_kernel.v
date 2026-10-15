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
 // Decode every cyclic predecessor set in parallel. Event IDs5/6 stay masked.
 wire[7:0]legal_available=available&8'h9f;
 wire[7:0]cursor_onehot=8'b1<<cursor;
 wire[7:0]grant;
 wire chosen=|grant;
 wire[7:0]eligible_0;
 assign eligible_0[0]=cursor_onehot[0] && !(|(legal_available & 8'h00));
 assign eligible_0[1]=cursor_onehot[1] && !(|(legal_available & 8'h9e));
 assign eligible_0[2]=cursor_onehot[2] && !(|(legal_available & 8'h9c));
 assign eligible_0[3]=cursor_onehot[3] && !(|(legal_available & 8'h98));
 assign eligible_0[4]=cursor_onehot[4] && !(|(legal_available & 8'h90));
 assign eligible_0[5]=cursor_onehot[5] && !(|(legal_available & 8'h80));
 assign eligible_0[6]=cursor_onehot[6] && !(|(legal_available & 8'h80));
 assign eligible_0[7]=cursor_onehot[7] && !(|(legal_available & 8'h80));
 assign grant[0]=legal_available[0] && |eligible_0;
 wire[7:0]eligible_1;
 assign eligible_1[0]=cursor_onehot[0] && !(|(legal_available & 8'h01));
 assign eligible_1[1]=cursor_onehot[1] && !(|(legal_available & 8'h00));
 assign eligible_1[2]=cursor_onehot[2] && !(|(legal_available & 8'h9d));
 assign eligible_1[3]=cursor_onehot[3] && !(|(legal_available & 8'h99));
 assign eligible_1[4]=cursor_onehot[4] && !(|(legal_available & 8'h91));
 assign eligible_1[5]=cursor_onehot[5] && !(|(legal_available & 8'h81));
 assign eligible_1[6]=cursor_onehot[6] && !(|(legal_available & 8'h81));
 assign eligible_1[7]=cursor_onehot[7] && !(|(legal_available & 8'h81));
 assign grant[1]=legal_available[1] && |eligible_1;
 wire[7:0]eligible_2;
 assign eligible_2[0]=cursor_onehot[0] && !(|(legal_available & 8'h03));
 assign eligible_2[1]=cursor_onehot[1] && !(|(legal_available & 8'h02));
 assign eligible_2[2]=cursor_onehot[2] && !(|(legal_available & 8'h00));
 assign eligible_2[3]=cursor_onehot[3] && !(|(legal_available & 8'h9b));
 assign eligible_2[4]=cursor_onehot[4] && !(|(legal_available & 8'h93));
 assign eligible_2[5]=cursor_onehot[5] && !(|(legal_available & 8'h83));
 assign eligible_2[6]=cursor_onehot[6] && !(|(legal_available & 8'h83));
 assign eligible_2[7]=cursor_onehot[7] && !(|(legal_available & 8'h83));
 assign grant[2]=legal_available[2] && |eligible_2;
 wire[7:0]eligible_3;
 assign eligible_3[0]=cursor_onehot[0] && !(|(legal_available & 8'h07));
 assign eligible_3[1]=cursor_onehot[1] && !(|(legal_available & 8'h06));
 assign eligible_3[2]=cursor_onehot[2] && !(|(legal_available & 8'h04));
 assign eligible_3[3]=cursor_onehot[3] && !(|(legal_available & 8'h00));
 assign eligible_3[4]=cursor_onehot[4] && !(|(legal_available & 8'h97));
 assign eligible_3[5]=cursor_onehot[5] && !(|(legal_available & 8'h87));
 assign eligible_3[6]=cursor_onehot[6] && !(|(legal_available & 8'h87));
 assign eligible_3[7]=cursor_onehot[7] && !(|(legal_available & 8'h87));
 assign grant[3]=legal_available[3] && |eligible_3;
 wire[7:0]eligible_4;
 assign eligible_4[0]=cursor_onehot[0] && !(|(legal_available & 8'h0f));
 assign eligible_4[1]=cursor_onehot[1] && !(|(legal_available & 8'h0e));
 assign eligible_4[2]=cursor_onehot[2] && !(|(legal_available & 8'h0c));
 assign eligible_4[3]=cursor_onehot[3] && !(|(legal_available & 8'h08));
 assign eligible_4[4]=cursor_onehot[4] && !(|(legal_available & 8'h00));
 assign eligible_4[5]=cursor_onehot[5] && !(|(legal_available & 8'h8f));
 assign eligible_4[6]=cursor_onehot[6] && !(|(legal_available & 8'h8f));
 assign eligible_4[7]=cursor_onehot[7] && !(|(legal_available & 8'h8f));
 assign grant[4]=legal_available[4] && |eligible_4;
 assign grant[5]=1'b0;
 assign grant[6]=1'b0;
 wire[7:0]eligible_7;
 assign eligible_7[0]=cursor_onehot[0] && !(|(legal_available & 8'h1f));
 assign eligible_7[1]=cursor_onehot[1] && !(|(legal_available & 8'h1e));
 assign eligible_7[2]=cursor_onehot[2] && !(|(legal_available & 8'h1c));
 assign eligible_7[3]=cursor_onehot[3] && !(|(legal_available & 8'h18));
 assign eligible_7[4]=cursor_onehot[4] && !(|(legal_available & 8'h10));
 assign eligible_7[5]=cursor_onehot[5] && !(|(legal_available & 8'h00));
 assign eligible_7[6]=cursor_onehot[6] && !(|(legal_available & 8'h00));
 assign eligible_7[7]=cursor_onehot[7] && !(|(legal_available & 8'h00));
 assign grant[7]=legal_available[7] && |eligible_7;
 always @*handler_id={grant[4]|grant[7],grant[2]|grant[3]|grant[7],grant[1]|grant[3]|grant[7]};
 wire[2:0]dispatch_next_cursor={grant[3]|grant[4],grant[1]|grant[2],grant[0]|grant[2]|grant[4]};
 assign handler_valid=!reset&&ce&&(phase==IDLE||phase==WAIT||phase==FAULT)&&chosen;
 assign command_capture=enabled&&phase==IDLE&&grant[4]&&command_valid;
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
   if(handler_valid)cursor<=dispatch_next_cursor;
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
