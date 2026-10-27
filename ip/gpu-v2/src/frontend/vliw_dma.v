// Single-outstanding SDRAM-to-VLIW loader. No compute CE, data FIFO or response ready.
// The caller grants an inactive page for the complete job. Config requires GPU drain.
// Reset must also reset the memory transport; abort is the local drain-safe cancellation.
module gpu_vliw_dma(
 input clk, reset, abort,
 input descriptor_valid, output descriptor_ready, input page_writable, gpu_drained,
 input descriptor_page, input [7:0] descriptor_tag,
 input [21:0] config_source, metadata_source, payload_source,
 input [8:0] config_destination, metadata_destination, payload_destination,
 input [10:0] config_bytes, payload_bytes,
 output request_valid, input request_ready, output [21:0] request_address,
 output [1:0] request_line_count_minus_1,
 input response_valid, response_last, response_error, input [63:0] response_data,
 output dma_we, output [9:0] dma_addr, output [63:0] dma_data,
 output completion_valid, input completion_ready, output completion_publish,
 output completion_page, output [7:0] completion_tag, output [3:0] completion_error,
 output busy
);
 localparam IDLE=0, SELECT_SEGMENT=1, REQUEST=2, RESPONSE=3, COMPLETE=4;
 reg [2:0] state;
 reg page; reg [7:0] tag; reg [3:0] error;
 reg [21:0] cfg_src, meta_src, pay_src, source;
 reg [8:0] cfg_dst, meta_dst, pay_dst, destination;
 reg [7:0] cfg_beats, pay_beats, left;
 reg [1:0] segment;
 reg [4:0] burst_beats, copy_beats, beat;
 function valid_segment;
  input [21:0] src; input [8:0] dst; input [10:0] bytes; input payload;
  reg [22:0] source_end; reg [9:0] destination_end;
  begin
   source_end={1'b0,src}+({12'd0,bytes}+23'd31)/23'd32*23'd16;
   destination_end={1'b0,dst}+{2'd0,bytes[10:3]};
   valid_segment=bytes==0||
    (bytes<=1024&&bytes[2:0]==0&&src[3:0]==0&&(!payload||src[5:4]==0)&&
     source_end<=23'd4194304&&destination_end<=10'd512);
  end
 endfunction
 function overlap;
  input [8:0] a,b; input [10:0] an,bn;
  reg [9:0] ae,be;
  begin
   ae={1'b0,a}+{2'd0,an[10:3]};be={1'b0,b}+{2'd0,bn[10:3]};
   overlap=an!=0&&bn!=0&&{1'b0,a}<be&&{1'b0,b}<ae;
  end
 endfunction
 wire legal_descriptor=valid_segment(config_source,config_destination,config_bytes,1'b0)&&
  valid_segment(metadata_source,metadata_destination,11'd32,1'b0)&&
  valid_segment(payload_source,payload_destination,payload_bytes,1'b1)&&
  !overlap(config_destination,metadata_destination,config_bytes,11'd32)&&
  !overlap(config_destination,payload_destination,config_bytes,payload_bytes)&&
  !overlap(metadata_destination,payload_destination,11'd32,payload_bytes);
 assign descriptor_ready=state==IDLE&&page_writable&&(config_bytes==0||gpu_drained)&&!reset;
 assign request_valid=state==REQUEST&&!reset;
 assign request_address=source;
 assign request_line_count_minus_1=left>=16&&source[5:0]==0?2'd3:
  left>=8&&source[4:0]==0?2'd1:2'd0;
 wire [4:0] selected_beats=request_line_count_minus_1==3?5'd16:
  request_line_count_minus_1==1?5'd8:5'd4;
 wire malformed=!response_error&&((response_last!=(beat==burst_beats-1'b1))||beat>=burst_beats);
 assign dma_we=state==RESPONSE&&response_valid&&error==0&&!abort&&!response_error&&!malformed&&beat<copy_beats&&!reset;
 assign dma_addr={1'b0,destination}+{5'd0,beat};
 assign dma_data=response_data;
 assign completion_valid=state==COMPLETE&&!reset;
 assign completion_publish=error==0;
 assign completion_page=page;assign completion_tag=tag;assign completion_error=error;
 assign busy=state!=IDLE&&!reset;
 reg [3:0] next_error;
 always @*begin
  next_error=error;
  if(state!=IDLE&&state!=COMPLETE)begin
   if(abort)next_error=next_error|4'd2;
   if(response_valid&&state!=RESPONSE)next_error=next_error|4'd8;
  end
  if(state==RESPONSE&&response_valid)begin
   if(response_error)next_error=next_error|4'd4;
   if(malformed)next_error=next_error|4'd8;
  end
 end
 always @(posedge clk)begin
  if(reset)begin
   state<=IDLE;page<=0;tag<=0;error<=0;segment<=0;source<=0;destination<=0;
   cfg_src<=0;meta_src<=0;pay_src<=0;cfg_dst<=0;meta_dst<=0;pay_dst<=0;
   cfg_beats<=0;pay_beats<=0;left<=0;burst_beats<=0;copy_beats<=0;beat<=0;
  end else begin
   error<=next_error;
   case(state)
    IDLE:if(descriptor_valid&&descriptor_ready)begin
     page<=descriptor_page;tag<=descriptor_tag;segment<=0;
     cfg_src<=config_source;meta_src<=metadata_source;pay_src<=payload_source;
     cfg_dst<=config_destination;meta_dst<=metadata_destination;pay_dst<=payload_destination;
     cfg_beats<=config_bytes[10:3];pay_beats<=payload_bytes[10:3];
     error<=(legal_descriptor?4'd0:4'd1)|(abort?4'd2:4'd0);
     state<=legal_descriptor&&!abort?SELECT_SEGMENT:COMPLETE;
    end
    SELECT_SEGMENT:begin
     if(next_error!=0)state<=COMPLETE;
     else begin
      state<=REQUEST;
      case(segment)
       0:begin
        if(cfg_beats!=0)begin source<=cfg_src;destination<=cfg_dst;left<=cfg_beats;end
        else begin segment<=1;source<=meta_src;destination<=meta_dst;left<=4;end
       end
       1:begin source<=meta_src;destination<=meta_dst;left<=4;end
       default:begin
        source<=pay_src;destination<=pay_dst;left<=pay_beats;
        if(pay_beats==0)state<=COMPLETE;
       end
      endcase
     end
    end
    REQUEST:if(request_ready)begin
     burst_beats<=selected_beats;
     copy_beats<=left<selected_beats?left[4:0]:selected_beats;
     beat<=0;state<=RESPONSE;
    end
    RESPONSE:if(response_valid)begin
     if(response_error||response_last)begin
      if(next_error!=0)state<=COMPLETE;
      else if(left==copy_beats)begin
       if(segment==2)state<=COMPLETE;
       else begin segment<=segment+1'b1;state<=SELECT_SEGMENT;end
      end else begin
       left<=left-copy_beats;source<=source+{15'd0,burst_beats,2'b00};
       destination<=destination+{4'd0,copy_beats};state<=REQUEST;
      end
     end else if(beat!=31)beat<=beat+1'b1;
    end
    COMPLETE:if(completion_ready)state<=IDLE;
    default:state<=IDLE;
   endcase
  end
 end
endmodule
