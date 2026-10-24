// Original implementation from the wire protocol; no external RTL used.
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
reg [4:0] count=0;
reg [7:0] shift=0;
reg result_valid=0, rejected=0;
(* syn_preserve=1 *) reg dat_meta=1, dat_sync=1;
wire busy=state!=IDLE;
wire write_selected=device_write_enable && device_index==5;
wire abort_now=write_selected && device_channel==0 && device_write_data==3;
always @* begin
 device_read_data=0;
 if(device_read_enable && device_index==5) case(device_channel)
  0: device_read_data={12'd0,rejected,!att,result_valid,busy};
  2: device_read_data={8'd0,shift};
  default: device_read_data=0;
 endcase
end
always @(posedge clk) begin
 dat_meta<=dat; dat_sync<=dat_meta;
 if(reset || abort_now) begin
  state<=IDLE; divider<=0; count<=0; shift<=0;
  ds2_clk<=1;cmd<=1;att<=1;result_valid<=0;rejected<=0;
  if(reset)begin dat_meta<=1;dat_sync<=1;end
 end else begin
  if(busy) begin
   if(divider!=0) divider<=divider-1'b1;
   else begin
    divider<=215;
    case(state)
     SETUP: state<=IDLE;
     FALL: begin ds2_clk<=0;cmd<=shift[0];state<=RISE;end
     RISE: begin
      ds2_clk<=1; shift<={dat_sync,shift[7:1]};
      if(count==7) begin state<=GUARD;count<=31;end
      else begin count<=count+1'b1;state<=FALL;end
     end
     GUARD: if(count==0)begin state<=IDLE;result_valid<=1;cmd<=1;end
            else count<=count-1'b1;
     RELEASE: begin att<=1;cmd<=1;state<=IDLE;end
     default: state<=IDLE;
    endcase
   end
  end
  if(write_selected) case(device_channel)
   0: case(device_write_data)
    1: if(!busy && att && !result_valid)begin
        att<=0;state<=SETUP;divider<=215;
       end else rejected<=1;
    2: if(!busy && !att && !result_valid)begin
        state<=RELEASE;divider<=215;
       end else rejected<=1;
    default: rejected<=1;
   endcase
   1: if(!busy && !att && !result_valid)begin
       shift<=device_write_data[7:0];count<=0;divider<=215;state<=FALL;
      end else rejected<=1;
   2: if(!busy && result_valid) result_valid<=0;else rejected<=1;
   3: if(device_write_data[0]) rejected<=0;
   default: rejected<=1;
  endcase
 end
end
endmodule
