// Fixed-schedule, latest-only DS2 receiver. All state uses the main clock.
// DIV_BITS=7 gives210.9375kHz serial and206Hz fallback polling at54MHz.
module DualShock2 #(parameter DIV_BITS=7)(
 input wire clk,reset,frame_event,
 input wire[2:0]device_index,input wire[3:0]device_channel,
 input wire device_read_enable,device_write_enable,
 input wire[15:0]device_write_data,input wire dat,
 output reg[15:0]device_read_data,
 output reg ds2_clk=1,cmd=1,att=1
);
localparam POLL=2'd0,ENTER=2'd1,MODE=2'd2,EXIT=2'd3;
localparam DIGITAL=2'd0,CONFIG=2'd1,ANALOG=2'd2,PRESSURE=2'd3;
reg[DIV_BITS+10:0]timebase=0;
wire[5:0]phase=timebase[DIV_BITS+:6];
wire[4:0]slot=timebase[DIV_BITS+6+:5];
wire tick=~|timebase[DIV_BITS-1:0];
// One byte-slot tick is8192 main clocks. The fixed Display period is
// 109.74451 slots. A complete9/21-byte packet takes9.5/21.5 slots.
// Count99/87 after the next slot tick: publication precedes the next frame
// by2003..10195 main clocks (37..189us), including slot quantization.
// This input is the Display's existing main-domain synchronized event.
reg[6:0]lead_slots=0;
reg phase_pending=0;
wire active=!att;
wire slot_tick=tick&&phase==0;
wire phase_start=slot_tick&&phase_pending&&lead_slots==0;
wire fallback_start=(~|timebase)&&(!phase_pending||lead_slots>32);
wire frame_start=!active&&(phase_start||fallback_start);
wire serial_window=slot!=0&&phase[5:4]==0;
wire sample_phase=!phase[0]&&((phase[5:4]==0&&phase[3:1]!=0)||phase==16);
wire sample=tick&&active&&slot!=0&&sample_phase;
wire byte_end=tick&&active&&slot!=0&&phase==16;
(* syn_preserve=1 *) reg dat_meta=1,dat_sync=1;
reg[7:0]shift=0;
reg cmd_phase=1;
wire[7:0]received={dat_sync,shift[7:1]};
reg[1:0]operation=POLL,kind=DIGITAL;
reg header_good=0,front=0,valid=0,read_changed=0;
wire is_digital=received==8'h41;
wire is_config=received==8'hf3;
wire is_analog=received==8'h73;
wire is_pressure=received==8'h79;
wire known_id=is_digital||is_config||is_analog||is_pressure;
wire last_slot=operation==ENTER ? slot==5 :
               (operation==POLL&&kind==PRESSURE ? slot==21 : slot==9);
wire finish=active&&tick&&phase==32&&((slot==3&&!header_good)||last_slot);
wire publish=finish&&operation==POLL&&header_good&&kind!=CONFIG;
wire invalidate=finish&&(!header_good||(operation==POLL&&kind==CONFIG));
wire begin_read=device_index==5&&device_write_enable&&device_channel[3];
// Reuse the background page for failure diagnosis. Slot1 retains raw FF until
// a complete poll overwrites it with LY at slot9; no extra header registers.
wire storing=byte_end&&(slot<=3||(operation==POLL&&kind!=CONFIG&&slot<=9))&&!reset;
wire[3:0]wa={!front,slot[2:0]};
wire[3:0]ra={front^device_channel[3],device_channel[2:0]};
wire[7:0]sample_q;
`ifdef PORTABLE
reg[7:0]samples[0:15];
always @(posedge clk)if(storing)samples[wa]<=received;
assign sample_q=samples[ra];
`else
genvar i;
generate for(i=0;i<2;i=i+1)begin:sample_nibbles
 RAM16SDP4 ram(.DO(sample_q[i*4+:4]),.DI(received[i*4+:4]),
              .WAD(wa),.RAD(ra),.WRE(storing),.CLK(clk));
end endgenerate
`endif
// Direct serial truth table; no TX byte register or general SPI interface.
wire[2:0]bit_index=phase[3:1];
wire mode_command=operation==MODE;
wire exit_command=operation==EXIT;
wire command_bit=(slot==1&&bit_index==0)||
 (slot==2&&((bit_index==0&&operation[0])||
           (bit_index==1&&!mode_command)||(bit_index==2&&mode_command)||bit_index==6))||
 (slot==4&&bit_index==0&&(operation[0]^operation[1]))||
 (slot==5&&mode_command&&bit_index[2:1]==0)||
 (slot>=5&&exit_command&&(bit_index[0]^bit_index[2]));
always @*begin
 device_read_data=0;
 if(device_index==5)begin
  if(!device_channel[3]||(!device_channel[2]&&(|device_channel[1:0])))
   device_read_data={8'd0,sample_q};
  else if(device_channel==12)
   device_read_data={slot,phase,operation,header_good,dat_sync,att};
  else device_read_data={14'd0,read_changed,valid};
 end
end
always @(posedge clk)begin
 if(reset)begin
  timebase<=0;dat_meta<=1;dat_sync<=1;shift<=0;
  lead_slots<=0;phase_pending<=0;
  operation<=POLL;kind<=DIGITAL;header_good<=0;
  front<=0;valid<=0;read_changed<=0;ds2_clk<=1;cmd_phase<=1;att<=1;cmd<=1;
 end else begin
  cmd<=cmd_phase;
  timebase<=timebase+1'b1;
  if(slot_tick&&phase_pending&&lead_slots!=0)lead_slots<=lead_slots-1'b1;
  if(frame_start)begin
   timebase<=1;
   if(phase_start)phase_pending<=0;
  end
  // A new beacon schedules the following frame; it never retimes an active
  // wire transaction. A missing beacon simply leaves autonomous fallback.
  if(frame_event)begin
   lead_slots<=(operation==POLL&&kind==PRESSURE)?7'd87:7'd99;
   phase_pending<=1;
  end
  dat_meta<=dat;dat_sync<=dat_meta;
  // CMD changes one main-clock after a rise, preserving sampling-edge hold.
  // DAT is sampled at the low-to-high event, before the CLK pin register.
  ds2_clk<=!(active&&serial_window&&phase[0]);
  if(tick&&active&&serial_window&&!phase[0])cmd_phase<=command_bit;
  if(sample)shift<=received;
  if(frame_start)begin att<=0;header_good<=1;kind<=DIGITAL;end
  if(byte_end)case(slot)
   1:header_good<=received==8'hff;
   2:begin
    header_good<=header_good&&known_id;
    kind<={!received[7]&&received[5],received[7]||received[3]};
   end
   3:header_good<=header_good&&received==8'h5a;
   default:begin end
  endcase
  if(finish)begin
   att<=1;cmd_phase<=1;
   case(operation)
    POLL:if(!header_good)operation<=POLL;
         else if(kind==DIGITAL)operation<=ENTER;
         else if(kind==CONFIG)operation<=EXIT;
         else operation<=POLL;
    ENTER:operation<=header_good?MODE:POLL;
    MODE:operation<=header_good?EXIT:POLL;
    EXIT:operation<=POLL;
   endcase
  end
  if(publish)begin front<=!front;valid<=1;end
  if(invalidate)valid<=0;
  if(begin_read)read_changed<=0;
  // Publication/invalidation wins a simultaneous CPU begin-read. No ABA loss.
  if(publish||(invalidate&&valid))read_changed<=1;
 end
end
`ifndef SYNTHESIS
always @(posedge clk)if(!reset)begin
 // Diagnostic high channels intentionally observe the writing page; they are
 // not coherent input. Normal low-channel reads must stay on the front page.
 if(storing&&!device_channel[3]&&wa==ra)$fatal(1,"DS2 snapshot read/write page collision");
 if((~|timebase)&&active)$fatal(1,"DS2 transaction exceeded the fixed frame");
end
initial if(DIV_BITS<2)$fatal(1,"DS2 divider must cover two DAT synchronizers");
`endif
endmodule
