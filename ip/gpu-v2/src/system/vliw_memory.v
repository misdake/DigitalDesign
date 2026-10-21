// Transaction boundary only: no compute CE, payload FIFO or SDRAM policy.
// Geometry and texture share RO; framebuffer read/write use independent masters.
module gpu_vliw_memory_router #(parameter CONTROL=0)(
 input clk,reset,
 input geometry_request_valid,output geometry_request_ready,
 input[21:0]geometry_request_address,input[1:0]geometry_request_count,
 output geometry_response_valid,geometry_response_last,geometry_response_error,
 output[63:0]geometry_response_data,
 input control_request_valid,output control_request_ready,input[21:0]control_request_address,input[1:0]control_request_count,
 output control_response_valid,control_response_last,control_response_error,output[63:0]control_response_data,
 input texture_request_valid,input[31:0]texture_request_address,
 output texture_request_ready,texture_read_valid,output[3:0]texture_read_index,
 output[63:0]texture_read_data,output texture_complete,texture_ok,
 input framebuffer_request_valid,framebuffer_request_write,input[31:0]framebuffer_request_address,
 output framebuffer_request_ready,input framebuffer_write_valid,input[63:0]framebuffer_write_data,
 output framebuffer_write_ready,framebuffer_read_valid,output[63:0]framebuffer_read_data,
 output framebuffer_complete,framebuffer_ok,
 output ro_request_valid,input ro_request_ready,output[21:0]ro_address,output[1:0]ro_count,
 input ro_response_valid,ro_response_last,ro_response_error,input[63:0]ro_data,
 output fb_r_request_valid,input fb_r_request_ready,output[21:0]fb_r_address,output[1:0]fb_r_count,
 input fb_r_response_valid,fb_r_response_last,fb_r_response_error,input[63:0]fb_r_data,
 output fb_w_request_valid,input fb_w_request_ready,output[21:0]fb_w_address,output[1:0]fb_w_count,
 output[63:0]fb_w_data,input fb_w_data_ready,
 input fb_w_response_valid,fb_w_response_last,fb_w_response_error,
 output busy,output reg fault
);
 localparam IDLE=0,REQUEST=1,ACTIVE=2,LOCAL_ERROR=3;
 reg[1:0]rs,fs;
 reg[1:0]owner,prefer_owner;reg ro_legal,fb_legal,fb_write;
 reg[21:0]ra,fa;reg[1:0]rc;
 reg[4:0]rb,fb;reg ro_bad,fb_bad;
 function legal_byte_sector;
  input[31:0]a;
  begin legal_byte_sector=a[31:23]==0&&a[6:0]==0;end
 endfunction
 function legal_halfword_burst;
  input[21:0]a;input[1:0]c;
  begin case(c)
   0:legal_halfword_burst=a[3:0]==0;
   1:legal_halfword_burst=a[4:0]==0;
   3:legal_halfword_burst=a[5:0]==0;
   default:legal_halfword_burst=0;
  endcase end
 endfunction
 reg[1:0]choose_owner;
 always @*begin
  choose_owner=0;
  case(prefer_owner)
   1:begin
    if(texture_request_valid)choose_owner=1;
    else if(CONTROL&&control_request_valid)choose_owner=2;
   end
   2:begin
    if(CONTROL&&control_request_valid)choose_owner=2;
    else if(!geometry_request_valid&&texture_request_valid)choose_owner=1;
   end
   default:begin
    if(!geometry_request_valid)begin
     if(texture_request_valid)choose_owner=1;
     else if(CONTROL&&control_request_valid)choose_owner=2;
    end
   end
  endcase
 end
 wire[4:0]ro_beats=rc==3?5'd16:rc==1?5'd8:5'd4;
 wire ro_terminal=ro_response_valid&&(ro_response_last||ro_response_error);
 wire ro_malformed=ro_response_valid&&!ro_response_error&&
  (rb>=ro_beats||(ro_response_last!=(rb==ro_beats-1'b1)));
 assign ro_request_valid=rs==REQUEST&&ro_legal&&!reset;
 assign ro_address=ra;assign ro_count=rc;
 assign geometry_request_ready=rs==REQUEST&&owner==0&&(ro_legal?ro_request_ready:1'b1)&&!reset;
 assign texture_request_ready=rs==REQUEST&&owner==1&&(ro_legal?ro_request_ready:1'b1)&&!reset;
 assign geometry_response_valid=!reset&&owner==0&&((rs==ACTIVE&&ro_response_valid)||rs==LOCAL_ERROR);
 assign geometry_response_last=rs==LOCAL_ERROR||(rs==ACTIVE&&ro_response_last);
 assign geometry_response_error=rs==LOCAL_ERROR||(rs==ACTIVE&&ro_response_error);
 assign geometry_response_data=ro_data;
 assign texture_read_valid=rs==ACTIVE&&owner==1&&ro_response_valid&&!ro_response_error&&rb<16&&!reset;
 assign texture_read_index=rb[3:0];assign texture_read_data=ro_data;
 assign control_request_ready=CONTROL&&rs==REQUEST&&owner==2&&(ro_legal?ro_request_ready:1'b1)&&!reset;
 assign control_response_valid=CONTROL&&!reset&&owner==2&&((rs==ACTIVE&&ro_response_valid)||rs==LOCAL_ERROR);
 assign control_response_last=rs==LOCAL_ERROR||(rs==ACTIVE&&ro_response_last);
 assign control_response_error=rs==LOCAL_ERROR||(rs==ACTIVE&&ro_response_error);
 assign control_response_data=ro_data;
 assign texture_complete=!reset&&owner==1&&((rs==ACTIVE&&ro_terminal)||rs==LOCAL_ERROR);
 assign texture_ok=rs==ACTIVE&&!ro_response_error&&!ro_bad&&!ro_malformed;

 // The framebuffer cache has one held transaction. Split its mode once here;
 // do not arbitrate it again with the RO source or buffer its payload.
 assign fb_r_request_valid=fs==REQUEST&&fb_legal&&!fb_write&&!reset;
 assign fb_w_request_valid=fs==REQUEST&&fb_legal&&fb_write&&framebuffer_write_valid&&!reset;
 assign fb_r_address=fa;assign fb_w_address=fa;
 assign fb_r_count=2'd3;assign fb_w_count=2'd3;
 assign fb_w_data=framebuffer_write_data;
 wire f_ready=fb_write?(fb_w_request_ready&&framebuffer_write_valid):fb_r_request_ready;
 assign framebuffer_request_ready=fs==REQUEST&&(fb_legal?f_ready:1'b1)&&!reset;
 wire fv=fb_write?fb_w_response_valid:fb_r_response_valid;
 wire fl=fb_write?fb_w_response_last:fb_r_response_last;
 wire fe=fb_write?fb_w_response_error:fb_r_response_error;
 wire f_terminal=fv&&(fl||fe);
 wire wbeat=fs==ACTIVE&&fb_write&&fb_w_data_ready&&fb<16&&!reset;
 wire rbeat=fs==ACTIVE&&!fb_write&&fv&&!fe&&fb<16&&!reset;
 wire[5:0]f_total={1'b0,fb}+{5'd0,wbeat}+{5'd0,rbeat};
 wire f_malformed=(!fb_write&&fv&&!fe&&(fb>=16||(fl!=(fb==15))))||
  (fb_write&&((wbeat&&!framebuffer_write_valid)||(fv&&!fe&&!fl)));
 assign framebuffer_write_ready=wbeat;
 assign framebuffer_read_valid=rbeat;
 assign framebuffer_read_data=fb_r_data;
 assign framebuffer_complete=!reset&&((fs==ACTIVE&&f_terminal)||fs==LOCAL_ERROR);
 assign framebuffer_ok=fs==ACTIVE&&!fe&&!fb_bad&&!f_malformed&&f_total==16;
 assign busy=rs!=IDLE||fs!=IDLE;
 always @(posedge clk)begin
  if(reset)begin
   rs<=IDLE;fs<=IDLE;owner<=0;prefer_owner<=0;ro_legal<=0;fb_legal<=0;fb_write<=0;
   ra<=0;fa<=0;rc<=0;rb<=0;fb<=0;ro_bad<=0;fb_bad<=0;fault<=0;
  end else begin
   case(rs)
    IDLE:if(geometry_request_valid||texture_request_valid||(CONTROL&&control_request_valid))begin
     rs<=REQUEST;owner<=choose_owner;rb<=0;ro_bad<=0;
     case(choose_owner)
      1:begin ra<=texture_request_address[22:1];rc<=3;ro_legal<=legal_byte_sector(texture_request_address);end
      2:begin ra<=control_request_address;rc<=control_request_count;ro_legal<=legal_halfword_burst(control_request_address,control_request_count);end
      default:begin ra<=geometry_request_address;rc<=geometry_request_count;ro_legal<=legal_halfword_burst(geometry_request_address,geometry_request_count);end
     endcase
    end
    REQUEST:if(geometry_request_ready||texture_request_ready||control_request_ready)begin
     rs<=ro_legal?ACTIVE:LOCAL_ERROR;
     if(CONTROL)prefer_owner<=owner==2?2'd0:owner+1'b1;
     else prefer_owner<=owner==0?2'd1:2'd0;
     if(!ro_legal)fault<=1;
    end
    ACTIVE:if(ro_response_valid)begin
     if(ro_malformed||ro_response_error)fault<=1;
     if(ro_malformed)ro_bad<=1;
     if(rb!=31)rb<=rb+1'b1;
     if(ro_terminal)rs<=IDLE;
    end
    LOCAL_ERROR:rs<=IDLE;
   endcase
   case(fs)
    IDLE:if(framebuffer_request_valid)begin
     fs<=REQUEST;fa<=framebuffer_request_address[22:1];fb_write<=framebuffer_request_write;
     fb_legal<=legal_byte_sector(framebuffer_request_address);fb<=0;fb_bad<=0;
    end
    REQUEST:if(framebuffer_request_ready)begin
     fs<=fb_legal?ACTIVE:LOCAL_ERROR;if(!fb_legal)fault<=1;
    end
    ACTIVE:begin
     if(wbeat||rbeat)fb<=fb+1'b1;
     if(f_malformed)begin fb_bad<=1;fault<=1;end
     if(fv&&fe)fault<=1;
     if(f_terminal)begin fs<=IDLE;if(f_total!=16&&!fe)fault<=1;end
    end
    LOCAL_ERROR:fs<=IDLE;
   endcase
   if(ro_response_valid&&rs!=ACTIVE)fault<=1;
   if((fb_r_response_valid&&(fs!=ACTIVE||fb_write))||
      (fb_w_response_valid&&(fs!=ACTIVE||!fb_write)))fault<=1;
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset)begin
  if(rs==REQUEST)begin
   if(owner==1)begin
    if(!texture_request_valid||texture_request_address[22:1]!=ra||
       legal_byte_sector(texture_request_address)!=ro_legal)$fatal(1,"texture presented request changed");
   end else if(owner==2)begin
    if(!control_request_valid||control_request_address!=ra||control_request_count!=rc)
     $fatal(1,"control presented request changed");
   end else if(!geometry_request_valid||geometry_request_address!=ra||geometry_request_count!=rc)
    $fatal(1,"geometry presented request changed");
  end
  if(fs==REQUEST&&(!framebuffer_request_valid||framebuffer_request_address[22:1]!=fa||
    framebuffer_request_write!=fb_write||legal_byte_sector(framebuffer_request_address)!=fb_legal))
   $fatal(1,"framebuffer presented request changed");
 end
`endif
endmodule

// Standalone factory: frozen geometry loader plus the three-master boundary.
module gpu_vliw_memory #(parameter CONTROL=0)(
 input clk,reset,geometry_abort,
 input descriptor_valid,output descriptor_ready,input page_writable,gpu_drained,
 input descriptor_page,input[7:0]descriptor_tag,
 input[21:0]config_source,metadata_source,payload_source,
 input[8:0]config_destination,metadata_destination,payload_destination,
 input[10:0]config_bytes,payload_bytes,
 output dma_we,output[9:0]dma_addr,output[63:0]dma_data,
 output completion_valid,input completion_ready,output completion_publish,completion_page,
 output[7:0]completion_tag,output[3:0]completion_error,
 input control_request_valid,output control_request_ready,input[21:0]control_request_address,input[1:0]control_request_count,
 output control_response_valid,control_response_last,control_response_error,output[63:0]control_response_data,
 input texture_request_valid,input[31:0]texture_request_address,
 output texture_request_ready,texture_read_valid,output[3:0]texture_read_index,
 output[63:0]texture_read_data,output texture_complete,texture_ok,
 input framebuffer_request_valid,framebuffer_request_write,input[31:0]framebuffer_request_address,
 output framebuffer_request_ready,input framebuffer_write_valid,input[63:0]framebuffer_write_data,
 output framebuffer_write_ready,framebuffer_read_valid,output[63:0]framebuffer_read_data,
 output framebuffer_complete,framebuffer_ok,
 output ro_request_valid,input ro_request_ready,output[21:0]ro_address,output[1:0]ro_count,
 input ro_response_valid,ro_response_last,ro_response_error,input[63:0]ro_data,
 output fb_r_request_valid,input fb_r_request_ready,output[21:0]fb_r_address,output[1:0]fb_r_count,
 input fb_r_response_valid,fb_r_response_last,fb_r_response_error,input[63:0]fb_r_data,
 output fb_w_request_valid,input fb_w_request_ready,output[21:0]fb_w_address,output[1:0]fb_w_count,
 output[63:0]fb_w_data,input fb_w_data_ready,
 input fb_w_response_valid,fb_w_response_last,fb_w_response_error,
 output busy,output fault
);
 wire grv,grr,gsv,gsl,gse;wire[21:0]gra;wire[1:0]grc;wire[63:0]gsd;
 wire loader_busy,router_busy;
 assign busy=loader_busy||router_busy;
 gpu_vliw_dma geometry(
 .clk(clk),.reset(reset),.abort(geometry_abort),
 .descriptor_valid(descriptor_valid),.descriptor_ready(descriptor_ready),.page_writable(page_writable),.gpu_drained(gpu_drained),
 .descriptor_page(descriptor_page),.descriptor_tag(descriptor_tag),
 .config_source(config_source),.metadata_source(metadata_source),.payload_source(payload_source),
 .config_destination(config_destination),.metadata_destination(metadata_destination),.payload_destination(payload_destination),
 .config_bytes(config_bytes),.payload_bytes(payload_bytes),
 .request_valid(grv),.request_ready(grr),.request_address(gra),.request_line_count_minus_1(grc),
 .response_valid(gsv),.response_last(gsl),.response_error(gse),.response_data(gsd),
 .dma_we(dma_we),.dma_addr(dma_addr),.dma_data(dma_data),
 .completion_valid(completion_valid),.completion_ready(completion_ready),.completion_publish(completion_publish),
 .completion_page(completion_page),.completion_tag(completion_tag),.completion_error(completion_error),.busy(loader_busy));
 gpu_vliw_memory_router #(.CONTROL(CONTROL)) router(
 .clk(clk),.reset(reset),.geometry_request_valid(grv),.geometry_request_ready(grr),
 .control_request_valid(control_request_valid),.control_request_ready(control_request_ready),
 .control_request_address(control_request_address),.control_request_count(control_request_count),
 .control_response_valid(control_response_valid),.control_response_last(control_response_last),
 .control_response_error(control_response_error),.control_response_data(control_response_data),
 .geometry_request_address(gra),.geometry_request_count(grc),.geometry_response_valid(gsv),
 .geometry_response_last(gsl),.geometry_response_error(gse),.geometry_response_data(gsd),
 .texture_request_valid(texture_request_valid),
 .texture_request_address(texture_request_address),
 .texture_request_ready(texture_request_ready),
 .texture_read_valid(texture_read_valid),
 .texture_read_index(texture_read_index),
 .texture_read_data(texture_read_data),
 .texture_complete(texture_complete),
 .texture_ok(texture_ok),
 .framebuffer_request_valid(framebuffer_request_valid),
 .framebuffer_request_write(framebuffer_request_write),
 .framebuffer_request_address(framebuffer_request_address),
 .framebuffer_request_ready(framebuffer_request_ready),
 .framebuffer_write_valid(framebuffer_write_valid),
 .framebuffer_write_data(framebuffer_write_data),
 .framebuffer_write_ready(framebuffer_write_ready),
 .framebuffer_read_valid(framebuffer_read_valid),
 .framebuffer_read_data(framebuffer_read_data),
 .framebuffer_complete(framebuffer_complete),
 .framebuffer_ok(framebuffer_ok),
 .ro_request_valid(ro_request_valid),
 .ro_request_ready(ro_request_ready),
 .ro_address(ro_address),
 .ro_count(ro_count),
 .ro_response_valid(ro_response_valid),
 .ro_response_last(ro_response_last),
 .ro_response_error(ro_response_error),
 .ro_data(ro_data),
 .fb_r_request_valid(fb_r_request_valid),
 .fb_r_request_ready(fb_r_request_ready),
 .fb_r_address(fb_r_address),
 .fb_r_count(fb_r_count),
 .fb_r_response_valid(fb_r_response_valid),
 .fb_r_response_last(fb_r_response_last),
 .fb_r_response_error(fb_r_response_error),
 .fb_r_data(fb_r_data),
 .fb_w_request_valid(fb_w_request_valid),
 .fb_w_request_ready(fb_w_request_ready),
 .fb_w_address(fb_w_address),
 .fb_w_count(fb_w_count),
 .fb_w_data(fb_w_data),
 .fb_w_data_ready(fb_w_data_ready),
 .fb_w_response_valid(fb_w_response_valid),
 .fb_w_response_last(fb_w_response_last),
 .fb_w_response_error(fb_w_response_error),
 .busy(router_busy),
 .fault(fault));
endmodule
