// Two ordered UI tasks own their atlas storage through the last write terminal.
// Grid and atlas DMA writes are independent of the single synchronous source Q.
module gpu_ui_grid(
 input clk,reset,ce,start,stop,input[15:0]grid_base,input[6:0]atlas_base,
 input[9:0]first_cell,last_cell,
 output reg active,output reg done,
 output cp_re,output[8:0]cp_addr,input cp_ready,input[63:0]cp_q,
 output work_valid,input work_ready,output[21:0]work_source,output[6:0]work_destination,
 input dma_idle,
 output cmd_valid,cmd_write,lower_ready,output[8:0]tile,input cmd_ready,cmd_done,
 output pixel_valid,pixel_half,output[4:0]pixel_word,output[63:0]pixels
);
 reg[6:0]atlas;
 reg[2:0]init_sector;
 reg initialized,scan_done;
 reg[8:0]scan;
 reg[63:0]window;reg[2:0]pairs;
 reg[1:0]valid,loaded;reg put,get,loader;
 reg[8:0]task_tile[0:1];reg[15:0]ids[0:1];
 reg[2:0]load_part;
 localparam WAIT_TASK=0,READ0=1,READ0_WAIT=2,READ1_WAIT=4,
  OVER0=5,WRITE0=6,WRITE0_WAIT=7;
 reg[3:0]state;
 reg overlay,overlay_half;reg[5:0]pixel;
 reg q_valid,q_pixel;reg q_half;reg[4:0]q_word;
 wire can_scan=initialized&&!scan_done;
 wire first_window=scan[8:2]==first_cell[9:3];
 wire before_first=first_window&&scan[1:0]<first_cell[2:1];
 wire first_pair=first_window&&scan[1:0]==first_cell[2:1];
 wire last_pair=scan==last_cell[9:1];
 wire[15:0]selected_ids={
  (before_first||(last_pair&&!last_cell[0]))?8'hff:window[15:8],
  (before_first||(first_pair&&first_cell[0]))?8'hff:window[7:0]};
 wire push=ce&&active&&!stop&&can_scan&&pairs!=0&&!valid[put]&&selected_ids!=16'hffff;
 wire scan_step=ce&&active&&!stop&&can_scan&&pairs!=0&&!valid[put];
 wire scan_read=can_scan&&pairs==0&&!q_valid&&!overlay;
 wire side=pixel[1];
 wire covered=side?(ids[get][15:8]!=8'hff):(ids[get][7:0]!=8'hff);
 wire pixel_read=overlay&&pixel<32&&covered;
 wire read_take=ce&&cp_re&&cp_ready;
 assign cp_re=active&&!stop&&(pixel_read||scan_read);
 assign cp_addr=overlay?{1'b1,get,side,overlay_half,pixel[4:2],pixel[0]}:{2'd0,scan[8:2]};
 assign pixel_valid=active&&ce&&q_valid&&q_pixel&&!stop;
 assign pixel_half=q_half;assign pixel_word=q_word;assign pixels=cp_q;
 wire init_work=!initialized&&init_sector!=7;
 wire atlas_work=initialized&&valid[loader]&&!loaded[loader]&&load_part<4;
 wire[7:0]char_id=load_part[1]?ids[loader][15:8]:ids[loader][7:0];
 wire skip_part=atlas_work&&char_id==8'hff;
 assign work_valid=active&&!stop&&(init_work||(atlas_work&&!skip_part));
 // The initial grid base dies before scanning starts; reuse window's low16.
 assign work_source=init_work?{window[15:0]+{13'd0,init_sector},6'd0}:{atlas,char_id,load_part[0],6'd0};
 assign work_destination=init_work?{2'd0,init_sector,2'd0}:{2'b01,loader,load_part[1:0],2'd0};
 wire work_take=ce&&work_valid&&work_ready;
 assign tile=task_tile[get];
 assign cmd_valid=active&&!stop&&(state==READ0||state==WRITE0);
 assign cmd_write=state==WRITE0;
 wire overlay_done=!overlay&&!(q_valid&&q_pixel);
 // An abort drains accepted writes with the existing background if necessary.
 assign lower_ready=stop||(overlay_half&&overlay_done);
 always @(posedge clk)begin
  if(reset)begin
   active<=0;done<=0;atlas<=0;init_sector<=0;initialized<=0;scan_done<=0;
   scan<=0;window<=0;pairs<=0;valid<=0;loaded<=0;put<=0;get<=0;loader<=0;
   load_part<=0;state<=WAIT_TASK;overlay<=0;overlay_half<=0;pixel<=0;
   q_valid<=0;q_pixel<=0;q_half<=0;q_word<=0;
   task_tile[0]<=0;task_tile[1]<=0;ids[0]<=0;ids[1]<=0;
  end else begin
   if(ce)begin
    q_valid<=read_take;q_pixel<=pixel_read;
    if(read_take)begin q_half<=overlay_half;q_word<=pixel[4:0];end
    if(q_valid&&!q_pixel)begin window<=cp_q;pairs<=4;end
    if(scan_step)begin
     scan<=scan+1'b1;window<={16'd0,window[63:16]};pairs<=pairs-1'b1;
     if(last_pair)scan_done<=1;
     if(push)begin valid[put]<=1;loaded[put]<=0;task_tile[put]<=scan;ids[put]<=selected_ids;put<=!put;end
    end
    if(active)begin
     if(work_take&&init_work)begin
      if(init_sector==last_cell[9:7])init_sector<=7;
      else init_sector<=init_sector+1'b1;
     end
     if(!initialized&&!init_work&&dma_idle)initialized<=1;
     if(atlas_work&&(skip_part||work_take))load_part<=load_part+1'b1;
     if(load_part==4&&dma_idle)begin loaded[loader]<=1;loader<=!loader;load_part<=0;end
     if(overlay&&pixel<32&&(!covered||read_take))pixel<=pixel+1'b1;
     if(overlay&&pixel==32)overlay<=0;
     case(state)
      WAIT_TASK:if(valid[get]&&loaded[get]&&(!(valid[!get]||push)||loaded[!get]))state<=READ0;
      OVER0:if(overlay_done)begin state<=WRITE0;overlay<=1;overlay_half<=1;pixel<=0;end
      default:begin end
     endcase
     if(initialized&&!can_scan&&valid==0&&dma_idle&&state==WAIT_TASK&&!q_valid)begin active<=0;done<=1;end
    end
   end
   // Local command admission and physical terminals follow wall time.
   if(cmd_valid&&cmd_ready)case(state)
    READ0:state<=READ0_WAIT;
    WRITE0:state<=WRITE0_WAIT;
    default:begin end
   endcase
   if(cmd_done)case(state)
    READ0_WAIT:begin state<=READ1_WAIT;overlay<=1;overlay_half<=0;pixel<=0;end
    READ1_WAIT:state<=OVER0;
    WRITE0_WAIT:begin valid[get]<=0;loaded[get]<=0;get<=!get;state<=WAIT_TASK;end
    default:begin end
   endcase
   if(active&&stop&&dma_idle&&cmd_ready)begin active<=0;done<=1;overlay<=0;q_valid<=0;end
   if(start)begin
    active<=1;done<=0;window[15:0]<=grid_base;atlas<=atlas_base;init_sector<=first_cell[9:7];initialized<=0;
    scan<={first_cell[9:3],2'd0};scan_done<=0;pairs<=0;valid<=0;loaded<=0;put<=0;get<=0;loader<=0;
    load_part<=0;state<=WAIT_TASK;overlay<=0;q_valid<=0;
   end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset)begin
  if(start&&active)$fatal(1,"UI start while active");
  if(pixel_valid&&(state!=READ1_WAIT&&state!=OVER0&&state!=WRITE0&&state!=WRITE0_WAIT))$fatal(1,"UI pixel lifetime");
 end
`endif
endmodule
