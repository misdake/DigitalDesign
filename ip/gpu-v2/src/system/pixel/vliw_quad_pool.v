// One authoritative producer pool. Physical32, active24, unpublished8.
// Producer and consumers share a clock, not their enables. Empty slots are dense.
module gpu_v2_vliw_quad_pool(
 input clk,reset,front_ce,backend_ce,input[1:0]phase,
 input draw_bank,draw_unlit,
 input quad_valid,output quad_ready,output[2:0]quad_id,
 input[7:0]quad_x,input[6:0]quad_y,input quad_rgb,
 input quad_end_valid,input[2:0]quad_end_id,input[3:0]quad_end_mask,
 output prefetch_valid,output[8:0]prefetch_tile,
 input result_we,input[6:0]result_wa,input[35:0]result_wd,
 input depth_event_valid,input[4:0]depth_event_pid,input[15:0]depth_event_z16,input depth_event_pass,
 input lod_valid,input[2:0]lod_id,input[7:0]lod_value,
 input publish_ready,output published,output[4:0]published_slot,output[22:0]published_begin,
 input[4:0]descriptor_issue_slot,output[22:0]descriptor_issue,descriptor_head,descriptor_next,
 output[8:0]descriptor_head_tile,descriptor_next_tile,
 input basic_read,input[6:0]basic_read_address,
 output[23:0]basic_return_data,output basic_collision,
 input depth_read,input[6:0]depth_read_address,output[15:0]depth_data,
 input light_ready,output light_valid,light_context_valid,
 output[35:0]light_normal,output[31:0]light_position,output reg[1:0]light_lane,
 output[9:0]light_descriptor,
 input sample_ready,output sample_valid,output[35:0]sample_data,output[1:0]sample_row,
 output sample_last,output[18:0]sample_descriptor,
 input retire_valid,input[4:0]retire_id,input empty_skip,
 output empty_head,empty_skip_ready,output[4:0]retire_slot,
 output[5:0]global_used,producer_used,light_pending,sample_pending,
 output busy,output reg fault
);
 reg[5:0]allocated,publish_ptr,retired,complete_tail,light_cursor,sample_cursor;
 reg[7:0]attr_pixel;
 reg[9:0]light_descriptor_q;reg[18:0]sample_descriptor_q;
 reg[5:0]normal_y_high;reg[3:0]depth_mask;
 assign global_used=allocated-retired;assign producer_used=allocated-publish_ptr;
 assign light_pending=publish_ptr-light_cursor;assign sample_pending=publish_ptr-sample_cursor;
 assign quad_ready=!reset&&!fault&&global_used<24&&producer_used<8;
 assign quad_id=allocated[2:0];wire allocate=front_ce&&quad_valid&&quad_ready;
 wire attr_event=front_ce&&result_we&&!reset;
 wire depth_event=front_ce&&depth_event_valid&&!reset;
 wire lod_event=front_ce&&lod_valid&&!reset;
 // Closed producer: RGB L arrives before W; UV L arrives after G/N/W.
 wire complete_event=(quad_rgb&&attr_event&&phase==0&&attr_pixel[1:0]==3)||(!quad_rgb&&lod_event);
 wire uv_lod_event=lod_event&&!quad_rgb;
 wire normal_write=attr_event&&phase[1];
 wire uv_write=attr_event&&(phase==3||phase==0)&&!quad_rgb;
 wire basic_write=attr_event&&(phase==3||phase==0);
 wire[8:0]basic_wa={2'd0,attr_pixel[6:0]};
 // DI is fixed wiring: only the column enables select the I return edge.
 // Unwritten columns need no phase-dependent data or zero selection.
 wire[35:0]normal_wd={result_wd[11:0],normal_y_high,result_wd[17:0]};
 wire[35:0]uv_wd={2'd0,result_wd[15:0],2'd0,result_wd[27:12]};
 wire[35:0]basic_wd={9'd0,1'b0,quad_rgb?result_wd[15:8]:8'hff,
  1'b0,quad_rgb?result_wd[7:0]:8'hff,1'b0,quad_rgb?result_wd[19:12]:8'hff};
 wire[3:0]normal_mask=phase[0]?4'b1100:4'b0011;
 wire[3:0]uv_mask=phase[0]?4'b0011:4'b1100;
 wire[3:0]basic_mask=phase[0]?4'b0001:4'b0110;
 wire basic_re=backend_ce&&basic_read&&!reset&&!fault;
 wire[35:0]normal_q,uv_q,basic_q;
 wire[3:0]next_depth_mask=(attr_pixel[1:0]==0?4'd0:depth_mask)|
  ((depth_event_pass?4'b0001:4'd0)<<attr_pixel[1:0]);
 wire mask_write=depth_event&&attr_pixel[1:0]==3;
 wire[18:0]header_write={quad_y,quad_x,1'b0,quad_rgb,draw_unlit,draw_bank};
 wire[18:0]issue_meta,light_meta,publish_meta;wire[2:0]sample_meta;
 wire[18:0]head_meta,next_meta;
 wire[4:0]next_slot=retired[4:0]+5'd1;
 // Keep tile identity as {Y4,X5}; only B maintenance needs linear addresses.
 wire[8:0]header_tile={quad_y[6:3],quad_x[7:3]};
 // G14 precedes coordinate advance on this edge. Hints may be dropped by the
 // cache, never allocate a pool owner, and have no ready path back to raster.
 assign prefetch_valid=front_ce&&quad_end_valid&&(|quad_end_mask)&&!reset&&!fault;
 assign prefetch_tile=header_tile;
 assign descriptor_head_tile={head_meta[18:15],head_meta[11:7]};
 assign descriptor_next_tile={next_meta[18:15],next_meta[11:7]};
 wire[3:0]issue_mask,head_mask,next_mask,light_mask,sample_mask,publish_mask;
 wire[7:0]sample_lod;
 // Every physical read view is paid, including independent publish/empty heads.
 gpu_v2_vliw_pool_meta #(.WIDTH(19)) mi(clk,reset,allocate,allocated[4:0],header_write,descriptor_issue_slot,issue_meta);
 gpu_v2_vliw_pool_meta #(.WIDTH(19)) mh(clk,reset,allocate,allocated[4:0],header_write,retired[4:0],head_meta);
 gpu_v2_vliw_pool_meta #(.WIDTH(19)) mn(clk,reset,allocate,allocated[4:0],header_write,next_slot,next_meta);
 gpu_v2_vliw_pool_meta #(.WIDTH(19)) ml(clk,reset,allocate,allocated[4:0],header_write,light_cursor[4:0],light_meta);
 gpu_v2_vliw_pool_meta #(.WIDTH(3)) ms(clk,reset,allocate,allocated[4:0],header_write[2:0],sample_cursor[4:0],sample_meta);
 gpu_v2_vliw_pool_meta #(.WIDTH(19)) mp(clk,reset,allocate,allocated[4:0],header_write,publish_ptr[4:0],publish_meta);
 gpu_v2_vliw_pool_meta #(.WIDTH(4)) fi(clk,reset,mask_write,attr_pixel[6:2],next_depth_mask,descriptor_issue_slot,issue_mask);
 gpu_v2_vliw_pool_meta #(.WIDTH(4)) fh(clk,reset,mask_write,attr_pixel[6:2],next_depth_mask,retired[4:0],head_mask);
 gpu_v2_vliw_pool_meta #(.WIDTH(4)) fn(clk,reset,mask_write,attr_pixel[6:2],next_depth_mask,next_slot,next_mask);
 gpu_v2_vliw_pool_meta #(.WIDTH(4)) fl(clk,reset,mask_write,attr_pixel[6:2],next_depth_mask,light_cursor[4:0],light_mask);
 gpu_v2_vliw_pool_meta #(.WIDTH(4)) fs(clk,reset,mask_write,attr_pixel[6:2],next_depth_mask,sample_cursor[4:0],sample_mask);
 gpu_v2_vliw_pool_meta #(.WIDTH(4)) fp(clk,reset,mask_write,attr_pixel[6:2],next_depth_mask,publish_ptr[4:0],publish_mask);
 gpu_v2_vliw_pool_meta #(.WIDTH(8)) ls(clk,reset,uv_lod_event,complete_tail[4:0],lod_value,sample_cursor[4:0],sample_lod);
 // Publication and the two parent descriptor views never arbitrate a read port.
 assign published_slot=publish_ptr[4:0];
 wire complete_head=publish_ptr!=complete_tail;
 assign published=backend_ce&&publish_ready&&complete_head&&!reset&&!fault;
 assign published_begin={publish_meta,publish_mask};
 assign descriptor_issue={issue_meta,issue_mask};assign descriptor_head={head_meta,head_mask};assign descriptor_next={next_meta,next_mask};
 assign retire_slot=retired[4:0];
 assign empty_head=publish_ptr!=retired&&head_mask==0&&!reset;
 assign empty_skip_ready=empty_head&&light_cursor!=retired&&sample_cursor!=retired&&!fault;
 wire skip_empty=backend_ce&&empty_skip&&empty_skip_ready;
 wire regular_retire=backend_ce&&retire_valid&&!reset&&!fault;

 localparam L_READ=0,L_X=1,L_CONTEXT=2,L_PIXEL=3;
 reg[1:0]light_state,sample_lane;reg[15:0]ndc_x;reg sample_loaded;
 wire light_exists=light_cursor!=publish_ptr;
 wire light_skip=light_state==L_READ&&light_exists&&(light_mask==0||light_meta[1]);
 wire light_fetch=light_state==L_READ&&light_exists&&!light_skip;
 wire light_read=backend_ce&&light_fetch&&!reset&&!fault;
 wire ndc_read=backend_ce&&(light_fetch||light_state==L_X)&&!reset&&!fault;
 wire[8:0]ndc_ra=light_state==L_X?9'd200+{2'd0,light_meta[18:12]}:{1'b0,light_meta[11:4]};
 wire[31:0]ndc_q;
 gpu_v2_vliw_ndc_rom ndc(clk,reset,ndc_read,ndc_ra,ndc_q);
 gpu_v2_vliw_pool_ram normal_bank(clk,reset,normal_write,normal_mask,{2'd0,attr_pixel[6:0]},
  normal_wd,light_read,{2'd0,light_cursor[4:0],light_lane},normal_q);
 // NormalQ and Y-ROM Q remain owned by this pixel through its acceptance.
 assign light_context_valid=!reset&&!fault&&(light_state==L_CONTEXT||light_state==L_PIXEL);
 assign light_valid=!reset&&!fault&&light_state==L_PIXEL;
 assign light_normal=normal_q;
 assign light_descriptor=light_descriptor_q;
 wire[15:0]ndc_y=light_lane[1]?ndc_q[31:16]:ndc_q[15:0];
 assign light_position={ndc_y,ndc_x};
 wire light_take=backend_ce&&light_valid&&light_ready;

 wire sample_exists=sample_cursor!=publish_ptr;
 wire sample_skip=!sample_loaded&&sample_lane==0&&sample_exists&&(sample_mask==0||sample_meta[2]);
 wire sample_fetch=backend_ce&&!sample_loaded&&sample_exists&&!sample_skip&&!reset&&!fault;
 gpu_v2_vliw_pool_ram uv_bank(clk,reset,uv_write,uv_mask,{2'd0,attr_pixel[6:0]},
  uv_wd,sample_fetch,{2'd0,sample_cursor[4:0],sample_lane},uv_q);
 assign sample_valid=sample_loaded&&!reset&&!fault;
 assign sample_data=uv_q;
 assign sample_row=sample_lane;assign sample_last=sample_lane==3;
 assign sample_descriptor=sample_descriptor_q;
 wire sample_take=backend_ce&&sample_valid&&sample_ready;
 gpu_v2_vliw_pool_ram basic_bank(clk,reset,basic_write,basic_mask,basic_wa,basic_wd,
  basic_re,{2'd0,basic_read_address},basic_q);
 assign basic_return_data={basic_q[25:18],basic_q[16:9],basic_q[7:0]};
 assign basic_collision=basic_re&&basic_write&&basic_wa=={2'd0,basic_read_address};
 // C acceptance pays this independent synchronous read. Q is held through D.
 gpu_v2_vliw_pool_depth_ram depth_bank(clk,reset,depth_event,attr_pixel[6:0],depth_event_z16,
  backend_ce&&depth_read&&!reset&&!fault,depth_read_address,depth_data);
 assign busy=allocated!=retired||light_state!=L_READ||sample_loaded;
 always @(posedge clk)begin
  if(reset)begin
   allocated<=0;publish_ptr<=0;retired<=0;complete_tail<=0;
   light_cursor<=0;sample_cursor<=0;attr_pixel<=0;
   light_descriptor_q<=0;sample_descriptor_q<=0;
   normal_y_high<=0;depth_mask<=0;light_state<=L_READ;light_lane<=0;ndc_x<=0;
   sample_loaded<=0;sample_lane<=0;fault<=0;
  end else begin
   // Metadata pays the same read edge as its payload; cursor changes cannot
   // propagate through consumers' public-credit/preflight/CE on this edge.
   if(light_read)light_descriptor_q<={light_meta[0],light_mask,light_cursor[4:0]};
   if(sample_fetch)sample_descriptor_q<={sample_lod,1'b0,sample_meta[0],sample_mask,sample_cursor[4:0]};
   if(allocate)allocated<=allocated+1'b1;
   if(complete_event)complete_tail<=complete_tail+1'b1;
   if(attr_event)begin
    if(phase==2)normal_y_high<=result_wd[23:18];
    if(phase==0)attr_pixel<=attr_pixel+1'b1;
   end
   if(depth_event)begin
    depth_mask<=next_depth_mask;
   end
   if(published)publish_ptr<=publish_ptr+1'b1;
   if(backend_ce&&!fault)begin
    if(light_skip)light_cursor<=light_cursor+1'b1;
    else case(light_state)
     L_READ:if(light_fetch)light_state<=L_X;
     L_X:begin ndc_x<=light_lane[0]?ndc_q[31:16]:ndc_q[15:0];light_state<=L_CONTEXT;end
     L_CONTEXT:light_state<=L_PIXEL;
     L_PIXEL:if(light_take)begin
      light_state<=L_READ;light_lane<=light_lane+1'b1;
      if(light_lane==3)light_cursor<=light_cursor+1'b1;
     end
    endcase
    if(sample_skip)sample_cursor<=sample_cursor+1'b1;
    if(sample_fetch)sample_loaded<=1;
    if(sample_take)begin
     sample_loaded<=0;sample_lane<=sample_lane+1'b1;
     if(sample_lane==3)sample_cursor<=sample_cursor+1'b1;
    end
   end
   if(regular_retire||skip_empty)begin
    if(retired==publish_ptr||(regular_retire&&retire_id!=retired[4:0])||
     light_cursor==retired||sample_cursor==retired)fault<=1;
    retired<=retired+1'b1;
   end
   if(backend_ce&&empty_skip&&!empty_skip_ready)fault<=1;
   if(regular_retire&&skip_empty||basic_collision)fault<=1;
  end
 end
`ifndef SYNTHESIS
 // Original completion facts and ID checks are simulation-only. Fixed producer.
 reg[7:0]depth_pixel;
 wire geom_event=front_ce&&quad_end_valid&&!reset;
 wire[5:0]attr_tail=attr_pixel[7:2],depth_tail=depth_pixel[7:2];
 reg[5:0]shadow_end_tail,shadow_lod_tail;reg[1:0]attr_word;
 reg shadow_rgb[0:31];reg shadow_geometry_seen[0:31],shadow_lod_seen[0:31];
 reg[3:0]shadow_depth_seen[0:31];reg[11:0]shadow_attributes[0:31];
 wire shadow_complete_head=publish_ptr!=allocated&&publish_ptr!=attr_tail&&publish_ptr!=depth_tail&&
  publish_ptr!=shadow_end_tail&&publish_ptr!=shadow_lod_tail;
 wire[5:0]shadow_unfinished=allocated-complete_tail,shadow_ready=complete_tail-publish_ptr;
 always @(posedge clk)begin
  if(reset)begin shadow_end_tail<=0;shadow_lod_tail<=0;attr_word<=0;depth_pixel<=0;end
  else begin
   if(allocate)begin
    shadow_rgb[allocated[4:0]]<=quad_rgb;
    shadow_geometry_seen[allocated[4:0]]<=0;shadow_lod_seen[allocated[4:0]]<=0;
    shadow_depth_seen[allocated[4:0]]<=0;shadow_attributes[allocated[4:0]]<=0;
   end
   if(geom_event)begin
    if(quad_end_id!=shadow_end_tail[2:0]||shadow_end_tail==allocated||phase!=2)$fatal(1,"shadow geometry prefix/order");
    shadow_end_tail<=shadow_end_tail+1'b1;shadow_geometry_seen[shadow_end_tail[4:0]]<=1;
   end
   if(lod_event)begin
    if(lod_id!=shadow_lod_tail[2:0]||shadow_lod_tail==allocated)$fatal(1,"shadow LOD prefix/order");
    shadow_lod_tail<=shadow_lod_tail+1'b1;shadow_lod_seen[shadow_lod_tail[4:0]]<=1;
   end
   if(attr_event)begin
    if(attr_word==2)attr_word<=0;else attr_word<=attr_word+1'b1;
    if(result_wa!={attr_pixel[4:0],attr_word}||attr_tail==allocated)$fatal(1,"shadow attributes prefix/order");
    if(shadow_attributes[attr_pixel[6:2]][attr_pixel[1:0]*3+attr_word])$fatal(1,"duplicate attribute word");
    shadow_attributes[attr_pixel[6:2]][attr_pixel[1:0]*3+attr_word]<=1;
   end
   if(depth_event)begin
    if(depth_pixel!=attr_pixel)$fatal(1,"depth/attribute source cursor diverged");
    depth_pixel<=depth_pixel+1'b1;
    if(depth_event_pid!=depth_pixel[4:0]||depth_tail==allocated||phase!=1)$fatal(1,"shadow depth prefix/order");
    if(shadow_depth_seen[depth_pixel[6:2]][depth_pixel[1:0]])$fatal(1,"duplicate depth event");
    shadow_depth_seen[depth_pixel[6:2]][depth_pixel[1:0]]<=1;
   end
   if(complete_event)begin
    if(complete_tail==allocated||shadow_rgb[complete_tail[4:0]]!=quad_rgb)$fatal(1,"completion exceeds allocation or mode changed");
    if(!shadow_geometry_seen[complete_tail[4:0]]||shadow_depth_seen[complete_tail[4:0]]!=4'b1111)$fatal(1,"completion before geometry/four depth events");
    if(quad_rgb)begin
     if(attr_tail!=complete_tail||attr_word!=2||attr_pixel[1:0]!=3||shadow_lod_tail==complete_tail)$fatal(1,"RGB completion before W or old L");
     if(shadow_attributes[complete_tail[4:0]]!=12'h7ff||!shadow_lod_seen[complete_tail[4:0]])$fatal(1,"RGB missing old eleven W or L");
    end else begin
     if(attr_tail==complete_tail||shadow_lod_tail!=complete_tail||lod_id!=complete_tail[2:0])$fatal(1,"UV completion before W or wrong old L");
     if(shadow_attributes[complete_tail[4:0]]!=12'hfff||shadow_lod_seen[complete_tail[4:0]])$fatal(1,"UV missing twelve W or duplicate L");
    end
   end
   if(complete_head!==shadow_complete_head)$fatal(1,"compressed and original publication barrier differ");
   if(shadow_unfinished>24||shadow_ready>24)$fatal(1,"compressed prefix overtook allocation/publication");
   if(sample_fetch&&sample_meta[2])$fatal(1,"RGB must skip LOD/UV consumer");
  end
 end
`endif
`ifndef SYNTHESIS
 // Front mode may change after all I attribute writes, even with old published
 // slots alive. All backend decode uses that slot's stored header instead.
 reg attribute_rgb;
 always @(posedge clk)begin
  if(reset)attribute_rgb<=0;
  else begin
   if(allocate&&attr_tail==allocated)attribute_rgb<=quad_rgb;
   if(allocate&&attr_tail!=allocated&&quad_rgb!=attribute_rgb)$fatal(1,"mode changed before I attribute drain");
   if(attr_event&&quad_rgb!=attribute_rgb)$fatal(1,"producer mode changed during I return");
  end
 end
 always @(posedge clk)if(!reset)begin
  if(global_used>24||producer_used>8)$fatal(1,"pool credit overflow");
  if(allocate&&(phase!=0||quad_x>=200||quad_y>=120))$fatal(1,"pool quad admission range/phase");
  if(attr_event&&((attr_word==0&&phase!=2)||(attr_word==1&&phase!=3)||(attr_word==2&&phase!=0)))$fatal(1,"pool attribute phase");
  if(attr_event&&!quad_rgb&&((attr_word==1&&|result_wd[29:28])||(attr_word==2&&|result_wd[17:16])))$fatal(1,"UV outside U16 source envelope");
  if(fault)$fatal(1,"pool protocol fault");
 end
`endif
endmodule

// Each instance is a real 1W1R bank. The caller gates its separate clock enables.
module gpu_v2_vliw_pool_ram(input clk,reset,we,input[3:0]wm,input[8:0]wa,input[35:0]wd,
 input re,input[8:0]ra,output[35:0]q);
`ifdef GPU_VLIW_POOL_PORTABLE
 reg[35:0]memory[0:511];reg[35:0]held;assign q=held;
 integer column;
 always @(posedge clk)begin
  if(we&&!reset)for(column=0;column<4;column=column+1)
   if(wm[column])memory[wa][column*9+:9]<=wd[column*9+:9];
  if(reset)held<=0;else if(re)held<=memory[ra];
 end
`else
 SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(0),.RESET_MODE("SYNC")) ram(
  .DO(q),.DI(wd),.ADA({wa,1'b0,wm}),.ADB({ra,5'b0}),.CLKA(clk),.CLKB(clk),
  .CEA(we&&!reset),.CEB(re&&!reset),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset),.BLKSELA(3'd0),.BLKSELB(3'd0));
`endif
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&we&&re&&wa==ra)$fatal(1,"pool physical bank collision");
`endif
endmodule

module gpu_v2_vliw_pool_depth_ram(input clk,reset,we,input[6:0]wa,input[15:0]wd,
 input re,input[6:0]ra,output[15:0]q);
`ifdef GPU_VLIW_POOL_PORTABLE
 reg[15:0]memory[0:127];reg[15:0]held;assign q=held;
 always @(posedge clk)begin
  if(we&&!reset)memory[wa]<=wd;
  if(reset)held<=0;else if(re)held<=memory[ra];
 end
`else
 wire[35:0]raw;assign q=raw[15:0];
 SDPX9B #(.BIT_WIDTH_0(18),.BIT_WIDTH_1(18),.READ_MODE(0),.RESET_MODE("SYNC")) ram(
  .DO(raw),.DI({20'd0,wd}),.ADA({3'd0,wa,4'b0011}),.ADB({3'd0,ra,4'b0000}),.CLKA(clk),.CLKB(clk),
  .CEA(we&&!reset),.CEB(re&&!reset),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset),.BLKSELA(3'd0),.BLKSELB(3'd0));
`endif
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&we&&re&&wa==ra)$fatal(1,"pool depth bank collision");
`endif
endmodule

// One asynchronous reader per explicit instance; 32 rows cost two16-row halves.
module gpu_v2_vliw_pool_meta #(parameter WIDTH=4)(input clk,reset,we,input[4:0]wa,
 input[WIDTH-1:0]wd,input[4:0]ra,output[WIDTH-1:0]q);
`ifdef GPU_VLIW_POOL_PORTABLE
 reg[WIDTH-1:0]memory[0:31];assign q=memory[ra];
 always @(posedge clk)if(we&&!reset)memory[wa]<=wd;
`else
 localparam LANES=(WIDTH+3)/4;wire[LANES*4-1:0]padded={{(LANES*4-WIDTH){1'b0}},wd};
 wire[LANES*4-1:0]lo,hi;genvar j;
 generate for(j=0;j<LANES;j=j+1)begin:lane
  RAM16SDP4 a(.DO(lo[j*4+:4]),.DI(padded[j*4+:4]),.WAD(wa[3:0]),.RAD(ra[3:0]),.WRE(we&&!reset&&!wa[4]),.CLK(clk));
  RAM16SDP4 b(.DO(hi[j*4+:4]),.DI(padded[j*4+:4]),.WAD(wa[3:0]),.RAD(ra[3:0]),.WRE(we&&!reset&&wa[4]),.CLK(clk));
 end endgenerate
 wire[LANES*4-1:0]selected=ra[4]?hi:lo;assign q=selected[WIDTH-1:0];
`endif
endmodule
