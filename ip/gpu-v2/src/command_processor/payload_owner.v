// Atomic semantic payload acquisition. Upper six input bits are canonical zero
// under the existing trusted command_validated preflight contract. No Context ABI.
// Scalar payload is retained here through command_done. Checked Context transport
// and actual DMA execution remain external. Fault retains ownership until reset.
module gpu_v2_command_processor(
 input wire clk,reset,ce,command_valid,
 input wire[95:0]command_payload,
 input wire command_dma,command_wait,command_draw,command_release,command_fence,command_unsupported,
 input wire[2:0]command_token,input wire command_validated,
 input wire[7:0]event_set,event_ack,input wire[3:0]dma_done,
 input wire dma_ready,draw_ready,draw_done,release_ready,fence_ready,external_fault,
 output wire[95:0]held_payload,output wire payload_owned,
 output wire command_capture,command_done,handler_valid,output wire[2:0]handler_id,
 output wire dma_valid,dma_accept,output wire[1:0]dma_token,
 output wire wait_ack,draw_valid,draw_accept,release_valid,release_accept,fence_ack,fault,
 output wire[2:0]phase_state,token_state,output wire[3:0]tokens_state,reserved_state,
 output wire[7:0]pending_state,output wire[2:0]cursor_state,output wire draw_return_state
);
 gpu_v2_command_control control(
 .clk(clk),
 .reset(reset),
 .ce(ce),
 .command_valid(command_valid),
 .command_dma(command_dma),
 .command_wait(command_wait),
 .command_draw(command_draw),
 .command_release(command_release),
 .command_fence(command_fence),
 .command_unsupported(command_unsupported),
 .command_token(command_token),
 .command_validated(command_validated),
 .event_set(event_set),
 .event_ack(event_ack),
 .dma_done(dma_done),
 .dma_ready(dma_ready),
 .draw_ready(draw_ready),
 .draw_done(draw_done),
 .release_ready(release_ready),
 .fence_ready(fence_ready),
 .external_fault(external_fault),
 .command_capture(command_capture),
 .command_done(command_done),
 .handler_valid(handler_valid),
 .handler_id(handler_id),
 .dma_valid(dma_valid),
 .dma_accept(dma_accept),
 .dma_token(dma_token),
 .wait_ack(wait_ack),
 .draw_valid(draw_valid),
 .draw_accept(draw_accept),
 .release_valid(release_valid),
 .release_accept(release_accept),
 .fence_ack(fence_ack),
 .fault(fault),
 .phase_state(phase_state),
 .token_state(token_state),
 .tokens_state(tokens_state),
 .reserved_state(reserved_state),
 .pending_state(pending_state),
 .cursor_state(cursor_state),
 .draw_return_state(draw_return_state));
 reg[89:0]payload;reg owned;
 assign held_payload={6'b0,payload};assign payload_owned=owned&&!reset;
 always @(posedge clk)begin
  if(reset)begin payload<=0;owned<=0;end
  else if(command_capture)begin payload<=command_payload[89:0];owned<=1;end
  else if(command_done)owned<=0;
 end
endmodule
