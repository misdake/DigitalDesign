// Original wire protocol engine. A single in-flight packet owns the snapshot.
module DualShock2(
 input wire clk, reset,
 input wire [2:0] device_index,
 input wire [3:0] device_channel,
 input wire device_read_enable, device_write_enable,
 input wire [15:0] device_write_data,
 input wire dat,
 output reg [15:0] device_read_data,
 output reg ds2_clk=1, cmd=1, att=1
);
localparam IDLE=3'd0, SETUP=3'd1, FALL=3'd2, RISE=3'd3,
           GUARD=3'd4, RELEASE=3'd5;
reg [2:0] state=IDLE;
reg [7:0] divider=0;
// Bits 0..7, then exactly 32 guard ticks from 8 through wrap to 7.
reg [4:0] count=0;
reg [7:0] shift=0;
reg [1:0] mode=0;
reg argument=0;
reg [4:0] byte_index=0;
reg [3:0] last_word=1;
// Bytes1..20 are ten packed words at addresses1..10 in 16x16 SSRAM.
// During collection the same byte_index becomes the CPU read cursor (2,4,...).
// The first FF header byte is represented by header_good, never published raw.
reg [15:0] packet [0:15];
reg [7:0] pair_high=0;
reg header_good=0;
reg result_valid=0, rejected=0;
(* syn_preserve=1 *) reg dat_meta=1, dat_sync=1;
wire busy=state!=IDLE;
wire write_selected=device_write_enable && device_index==5;
wire abort_now=write_selected && device_channel==0 && device_write_data==3;
wire [7:0] received={dat_sync,shift[7:1]};
reg [7:0] next_tx;
always @* begin
 next_tx=0;
 case(byte_index)
  0: next_tx={5'b01000,mode[1],!mode[1],mode[0]};
  2: next_tx={7'd0,argument};
  3: if(mode==2) next_tx=3;
     else if(mode==1 && !argument) next_tx=8'h5a;
  default: if(byte_index>=4 && mode==1 && !argument) next_tx=8'h5a;
 endcase
end
always @* begin
 device_read_data=0;
 if(device_read_enable && device_index==5) case(device_channel)
  0: device_read_data={11'd0,header_good,rejected,!att,result_valid,busy};
  1: device_read_data=packet[byte_index[4:1]];
  default: device_read_data=0;
 endcase
end
always @(posedge clk) begin
 dat_meta<=dat; dat_sync<=dat_meta;
 if(reset || abort_now) begin
  state<=IDLE; divider<=0; count<=0; shift<=0;
  mode<=0;argument<=0;byte_index<=0;last_word<=1;header_good<=0;
  ds2_clk<=1;cmd<=1;att<=1;result_valid<=0;rejected<=0;
  if(reset)begin dat_meta<=1;dat_sync<=1;end
 end else begin
  if(state==RISE && divider==0 && count==7 && !byte_index[0])
   packet[byte_index[4:1]]<={pair_high,received};
  if(busy) begin
   if(divider!=0) divider<=divider-1'b1;
   else begin
    divider<=215;
    case(state)
     SETUP: begin shift<=1;state<=FALL;end
     FALL: begin ds2_clk<=0;cmd<=shift[0];state<=RISE;end
     RISE: begin
      ds2_clk<=1; shift<=received;
      if(count==7) begin
       if(byte_index==0)header_good<=received==8'hff;
       if(byte_index[0])pair_high<=received;
       // Only bounded lengths; the CPU still validates the *entire* ID/header.
       if(byte_index==1) case(received[3:0])
        1: last_word<=2;
        3: last_word<=4;
        9: last_word<=10;
        default: last_word<=1;
       endcase
       state<=GUARD;count<=count+1'b1;
      end else begin count<=count+1'b1;state<=FALL;end
     end
     GUARD: if(count==7)begin
              cmd<=1;count<=0;
              if(byte_index[4:1]==last_word) state<=RELEASE;
              else begin byte_index<=byte_index+1'b1;shift<=next_tx;state<=FALL;end
            end else count<=count+1'b1;
     RELEASE: begin
      att<=1;cmd<=1;state<=IDLE;result_valid<=1;
      byte_index<=2;
     end
     default: state<=IDLE;
    endcase
   end
  end
  if(write_selected) case(device_channel)
   0: case(device_write_data)
    1: if(!busy && result_valid)begin
        byte_index<=byte_index+5'd2;
        if(byte_index[4:1]==last_word)result_valid<=0;
       end else rejected<=1;
    2: rejected<=0;
    default: rejected<=1;
   endcase
   1: if(!busy && !result_valid && device_write_data[1:0]!=3)begin
       mode<=device_write_data[1:0];argument<=device_write_data[8];
       byte_index<=0;last_word<=1;count<=0;divider<=215;header_good<=0;
       att<=0;state<=SETUP;
      end else rejected<=1;
   default: rejected<=1;
  endcase
 end
end
endmodule
