// All traffic and timestamps are in 54 MHz logic clocks. No cache/core shortcuts.
module SdramTrafficProbe (
 input wire clk, input wire [1:0] buttons,
 input wire [63:0] sdram_read_data, input wire sdram_read_valid,
 input wire sdram_init_done, input wire sdram_request_ready,
 input wire sdram_stream_active, input wire sdram_done, input wire sdram_write_data_ready,
 output wire [5:0] leds, output reg uart_tx=1,
 output wire sdram_next_valid, output wire [20:0] sdram_next_address,
 output wire sdram_request_valid,output wire sdram_write,
 output wire [20:0] sdram_address,output wire [3:0] sdram_write_mask,
 output wire [63:0] sdram_write_data,output wire sdram_write_data_valid,
 output wire [5:0] sdram_words
);
 localparam WINDOW=__WINDOW__, UART_DIV=__UART_DIV__;
 localparam EARLY_GRANT=__EARLY_GRANT__;
 localparam CHAIN_GROUP_FOUR=__CHAIN_GROUP_FOUR__;
 localparam FILL=0, VERIFY=1, RUN=2, DRAIN=3, REPORT=4, CLEAR=5, STOP=6;
 reg [2:0] state=FILL;
 wire reset=(|buttons)||!sdram_init_done;
 reg [3:0] mode=0;
 reg [31:0] tick=0, elapsed=0, measured_elapsed=0, busy_cycles=0;
 reg [1:0] cpu_write_phase=0;
 reg [3:0] missed_this_cycle;
 reg [31:0] read_bytes=0,write_bytes=0,groups=0,group_max=0,missed=0;
 // One outstanding job per client bounds each sum by elapsed, except a
 // display batch (at most 50*elapsed). WINDOW <= 2^24 fits u32, including drain.
 reg [31:0] group_sum=0;
 reg [31:0] group_stamp=0;
 reg failed=0, memory_busy=0, memory_reserved=0;
 reg report_done=0,report_active=0;
 reg [6:0] pending=0,active=0,writes=0;
 reg [6:0] reserved=0;
 reg [21:0] next_gpu_address=0;
 reg [31:0] next_gpu_arrival=0, next_gpu_grant=0;
 wire lookahead_window, lookahead_enable;
 assign lookahead_enable=EARLY_GRANT && lookahead_window;
 reg [21:0] addresses[0:6];
 reg [6:0] feed[0:6],received[0:6];
 reg [31:0] arrival[0:6],grant[0:6],timer[0:6],request_number[0:6];
 reg [5:0] display_left=0;
 reg [31:0] display_stamp=0;
 reg [5:0] fill_sector=0;
 reg [1:0] gpu_sector=0;
 reg gpu_writing=0;
 reg [31:0] gpu_group=0;
 reg [31:0] requests[0:6],completed[0:6];
 reg [31:0] wait_sum[0:6],first_sum[0:6],total_sum[0:6];
 reg [31:0] max_wait[0:6],max_first[0:6],max_total[0:6],min_first[0:6],min_total[0:6];
 wire [6:0] request_ready,feed_ready,response_valid,response_last,response_error;
 wire [63:0] response_data[0:6];
 wire [15:0] dma_result;
 wire [63:0] payload[0:6];
 wire [21:0] request_address[0:6];
 wire [15:0] dma_payload;
 wire memory_request_valid,memory_write,memory_line,memory_request_ready,memory_write_data_ready;
 wire [21:0] memory_address;
 wire [1:0] memory_line_count_minus_1;
 wire [63:0] memory_write_data,memory_read_data;
 wire memory_response_ready,memory_response_valid,memory_response_last,memory_error;
 assign feed_ready[0]=0;assign feed_ready[1]=0;assign feed_ready[3]=0;
 assign response_last[1]=received[1]==3;
 assign response_last[2]=writes[2]||received[2]==3;
 assign response_last[3]=1;
 assign response_data[3]={48'd0,dma_result};
 // synthesis translate_off
 integer early_grants=0;
 always @(posedge clk) if(!reset && |(request_ready & active)) early_grants=early_grants+1;
 // synthesis translate_on

 assign leds=~{failed,state==STOP,mode};
 function [31:0] pattern;
  input [21:0] halfword;
  reg [31:0] value;
  begin
   // Spread address-dependent stimulus over DQ, so the probe exercises every
   // data pin and does not turn high data lanes into fixed constants.
   value=32'h1eaf6000^{11'd0,halfword[21:1]};
   value=value^(value<<13);value=value^(value>>17);value=value^(value<<5);
   pattern=value;
  end
 endfunction
 genvar g;
 generate for(g=0;g<7;g=g+1)begin: data_sources
  assign request_address[g]=(g>=5 && active[g] && pending[g])?next_gpu_address:addresses[g];
  assign payload[g]={pattern(addresses[g]+feed[g]*4+2),pattern(addresses[g]+feed[g]*4)};
 end endgenerate
 wire [31:0] dma_word=pattern(addresses[3]);
 assign dma_payload=addresses[3][0]?dma_word[31:16]:dma_word[15:0];
 __ARBITER__ arbiter (
 __CLIENT_CONNECTIONS__
 );
 __SHARED_PORT__ #(.EARLY_GRANT(EARLY_GRANT), .CHAIN_GROUP_FOUR(CHAIN_GROUP_FOUR)) adapter (
 .clk(clk),.reset(reset),.cpu_request_valid(memory_request_valid),.cpu_write(memory_write),
 .cpu_line(memory_line),.cpu_address(memory_address),.cpu_line_count_minus_1(memory_line_count_minus_1),
 .cpu_write_data(memory_write_data),.cpu_response_ready(memory_response_ready),
 .controller_read_data(sdram_read_data),.controller_read_valid(sdram_read_valid),
 .controller_init_done(sdram_init_done),.controller_request_ready(sdram_request_ready),
 .controller_stream_active(sdram_stream_active),.cpu_lookahead_window(lookahead_window),
 .controller_next_valid(sdram_next_valid),.controller_next_address(sdram_next_address),
 .controller_done(sdram_done),.controller_write_data_ready(sdram_write_data_ready),
 .cpu_request_ready(memory_request_ready),.cpu_write_data_ready(memory_write_data_ready),
 .cpu_response_valid(memory_response_valid),.cpu_read_data(memory_read_data),
 .cpu_response_last(memory_response_last),.cpu_error(memory_error),
 .controller_request_valid(sdram_request_valid),.controller_write(sdram_write),
 .controller_address(sdram_address),.controller_write_mask(sdram_write_mask),
 .controller_write_data(sdram_write_data),.controller_write_data_valid(sdram_write_data_valid),.controller_words(sdram_words)
 );
 function enabled;
 input integer client;
 begin
  case(client)
   0:enabled=mode==4||mode==6||mode==7||mode==8;
   1,2,3,4:enabled=mode==5||mode==6||mode==7||mode==8;
   default:enabled=1;
  endcase
 end endfunction
 function [31:0] period;
 input integer client;
 begin
  case(client)
   0:period=mode==7?7500:150;
   1:period=mode==8?1:1728;
   2:period=mode==8?1:288;
   3:period=4096;
   4:period=mode==8?1:512;
   default:period=1;
  endcase
 end endfunction
 function [21:0] gpu_address;
 input [31:0] group_number;
 input [1:0] sector;
 begin
  case(mode)
   2:gpu_address={9'd0,group_number[3:0],sector,6'd0};
   3:gpu_address={10'd0,group_number[0],11'd0}+sector*64;
   default:gpu_address=sector*64;
  endcase
 end endfunction
 integer i;
 reg [63:0] expected_data;
 reg [31:0] wait_latency,first_latency,total_latency;
 always @(posedge clk)begin
  if(reset)begin
   state<=FILL;mode<=0;tick<=0;elapsed<=0;pending<=0;active<=0;writes<=0;
   fill_sector<=0;gpu_sector<=0;gpu_group<=0;gpu_writing<=0;display_left<=0;
   failed<=0;memory_busy<=0;memory_reserved<=0;reserved<=0;
   next_gpu_address<=0;next_gpu_arrival<=0;next_gpu_grant<=0;
   busy_cycles<=0;read_bytes<=0;write_bytes<=0;groups<=0;group_sum<=0;group_max<=0;missed<=0;
   for(i=0;i<7;i=i+1)begin
    addresses[i]<=0;feed[i]<=0;received[i]<=0;arrival[i]<=0;grant[i]<=0;timer[i]<=0;request_number[i]<=0;
    requests[i]<=0;completed[i]<=0;wait_sum[i]<=0;first_sum[i]<=0;total_sum[i]<=0;
    max_wait[i]<=0;max_first[i]<=0;max_total[i]<=0;min_first[i]<=32'hffffffff;min_total[i]<=32'hffffffff;
   end
  end else begin
   missed_this_cycle=0;
   tick<=tick+1;
   if(state==RUN||state==DRAIN)begin
    elapsed<=elapsed+1;
    if(memory_busy)busy_cycles<=busy_cycles+1;
   end
   if(memory_request_valid&&memory_request_ready)begin
    if(memory_busy)memory_reserved<=1;else memory_busy<=1;
   end
   if(memory_response_valid&&memory_response_ready&&memory_response_last)begin
    memory_busy<=memory_reserved;memory_reserved<=0;
   end
   for(i=0;i<7;i=i+1)begin
    if(request_ready[i])begin
     pending[i]<=0;
     if(active[i])begin reserved[i]<=1;next_gpu_grant<=tick;end
     else begin active[i]<=1;feed[i]<=0;received[i]<=0;grant[i]<=tick;end
     if(state==RUN||state==DRAIN)begin
      requests[i]<=requests[i]+1;
      wait_latency=tick-(active[i]?next_gpu_arrival:arrival[i]);wait_sum[i]<=wait_sum[i]+wait_latency;
      if(wait_latency>max_wait[i])max_wait[i]<=wait_latency;
     end
    end
    if(feed_ready[i])feed[i]<=feed[i]+1;
    if(response_valid[i])begin
     if(response_error[i])failed<=1;
     if(!writes[i])begin
      expected_data={pattern(addresses[i]+received[i]*4+2),pattern(addresses[i]+received[i]*4)};
      if(i==3)expected_data=addresses[i][0]?{48'd0,expected_data[31:16]}:{48'd0,expected_data[15:0]};
      if(response_data[i]!==expected_data)begin
       failed<=1;
       // synthesis translate_off
       $display("PATTERN mode=%0d client=%0d address=%h beat=%0d actual=%h expected=%h",mode,i,addresses[i],received[i],response_data[i],expected_data);
       // synthesis translate_on
      end
      received[i]<=received[i]+1;
     end
     if(state==RUN||state==DRAIN)begin
      if(received[i]==0)begin
       first_latency=tick-arrival[i];first_sum[i]<=first_sum[i]+first_latency;
       if(first_latency>max_first[i])max_first[i]<=first_latency;
       if(first_latency<min_first[i])min_first[i]<=first_latency;
      end
      if(!writes[i])read_bytes<=read_bytes+(i==3?2:8);
     end
     if(response_last[i])begin
      active[i]<=reserved[i];reserved[i]<=0;
      if(i>=5 && pending[i])begin addresses[i]<=next_gpu_address;arrival[i]<=next_gpu_arrival;end
      if(reserved[i])begin addresses[i]<=next_gpu_address;arrival[i]<=next_gpu_arrival;grant[i]<=next_gpu_grant;feed[i]<=0;received[i]<=0;end
      request_number[i]<=request_number[i]+1;
      if(i==2)cpu_write_phase<=cpu_write_phase==2?0:cpu_write_phase+1;
      if(state==RUN||state==DRAIN)begin
       completed[i]<=completed[i]+1;total_latency=tick-arrival[i];total_sum[i]<=total_sum[i]+total_latency;
       if(total_latency>max_total[i])max_total[i]<=total_latency;
       if(total_latency<min_total[i])min_total[i]<=total_latency;
       if(writes[i])write_bytes<=write_bytes+(i==3?2:i<4?32:i>=5&&CHAIN_GROUP_FOUR?512:128);
      end
      if(i==5||i==6)begin
       if(state==FILL||state==VERIFY)begin
        if(fill_sector==63)begin fill_sector<=0;state<=state==FILL?VERIFY:CLEAR;end
        else fill_sector<=fill_sector+1;
       end else begin
        gpu_sector<=CHAIN_GROUP_FOUR?0:gpu_sector+1;
        if(gpu_sector==3 || CHAIN_GROUP_FOUR)begin
         gpu_group<=gpu_group+1;gpu_writing<=mode>=5?!gpu_writing:mode==1;
         groups<=groups+1;group_sum<=group_sum+(tick-group_stamp);
         if(tick-group_stamp>group_max)group_max<=tick-group_stamp;
        end
       end
      end
     end
    end
   end
   if(EARLY_GRANT && !CHAIN_GROUP_FOUR && (state==RUN||state==DRAIN) && gpu_sector!=3 &&
      !pending[5]&&!pending[6]&&!reserved[5]&&!reserved[6]&&(active[5]||active[6])&&!response_last[5]&&!response_last[6])begin
    next_gpu_address<=gpu_address(gpu_group,gpu_sector+1'b1);next_gpu_arrival<=tick;
    if(active[5])pending[5]<=1;else pending[6]<=1;
   end
   case(state)
    FILL,VERIFY:if(!pending[5]&&!active[5]&&!pending[6]&&!active[6])begin
     if(state==FILL)begin pending[6]<=1;writes[6]<=1;addresses[6]<={10'd0,fill_sector,6'd0};end
     else begin pending[5]<=1;writes[5]<=0;addresses[5]<={10'd0,fill_sector,6'd0};end
    end
    CLEAR:begin
     elapsed<=0;busy_cycles<=0;read_bytes<=0;write_bytes<=0;groups<=0;group_sum<=0;group_max<=0;missed<=0;
     gpu_group<=0;gpu_sector<=0;gpu_writing<=mode==1;display_left<=0;
     cpu_write_phase<=0;
     for(i=0;i<7;i=i+1)begin
      requests[i]<=0;completed[i]<=0;wait_sum[i]<=0;first_sum[i]<=0;total_sum[i]<=0;
      max_wait[i]<=0;max_first[i]<=0;max_total[i]<=0;min_first[i]<=32'hffffffff;min_total[i]<=32'hffffffff;
      timer[i]<=0;request_number[i]<=0;
     end
     state<=RUN;
    end
    RUN:begin
     if(elapsed==WINDOW-1)state<=DRAIN;
     for(i=0;i<5;i=i+1)begin
      if(timer[i]!=0)timer[i]<=timer[i]-1;
      else if(enabled(i))begin
       timer[i]<=period(i)-1;
       if(i==0)begin
        if(display_left==0)begin display_left<=mode==7?50:1;display_stamp<=tick;end
        else missed_this_cycle=missed_this_cycle+1;
       end else if(!pending[i]&&!active[i])begin
        pending[i]<=1;arrival[i]<=tick;
        addresses[i]<=22'h800+(i==4?request_number[i][4:0]*64:request_number[i][6:0]*16);
        writes[i]<=i==2&&cpu_write_phase==2||i==3&&request_number[i][0];
       end else missed_this_cycle=missed_this_cycle+1;
      end
     end
     if(display_left!=0&&!pending[0]&&!active[0])begin
      pending[0]<=1;addresses[0]<=request_number[0][7:0]*16;arrival[0]<=display_stamp;display_left<=display_left-1;
     end
     if(!pending[5]&&!active[5]&&!pending[6]&&!active[6])begin
      if(gpu_writing)begin pending[6]<=1;writes[6]<=1;addresses[6]<=gpu_address(gpu_group,gpu_sector);arrival[6]<=tick;end
      else begin pending[5]<=1;writes[5]<=0;addresses[5]<=gpu_address(gpu_group,gpu_sector);arrival[5]<=tick;end
      if(gpu_sector==0)group_stamp<=tick;
     end
    end
    DRAIN:begin
     if(display_left!=0&&!pending[0]&&!active[0])begin
      pending[0]<=1;addresses[0]<=request_number[0][7:0]*16;arrival[0]<=display_stamp;display_left<=display_left-1;
     end
     if(gpu_sector!=0&&!pending[5]&&!active[5]&&!pending[6]&&!active[6])begin
      if(gpu_writing)begin pending[6]<=1;writes[6]<=1;addresses[6]<=gpu_address(gpu_group,gpu_sector);arrival[6]<=tick;end
      else begin pending[5]<=1;writes[5]<=0;addresses[5]<=gpu_address(gpu_group,gpu_sector);arrival[5]<=tick;end
     end
     if(pending==0&&active==0&&!memory_busy&&display_left==0&&gpu_sector==0)begin measured_elapsed<=elapsed;state<=REPORT;end
    end
    REPORT:if(report_done)begin if(failed)state<=STOP;else begin mode<=mode==8?0:mode+1;state<=CLEAR;end end
    default:;
   endcase
   // Freeze a diagnosable failure instead of waiting forever for a lost beat.
   if((state==RUN||state==DRAIN)&&elapsed>WINDOW+65536)begin failed<=1;measured_elapsed<=elapsed;state<=REPORT;end
   if(missed_this_cycle!=0)missed<=missed+missed_this_cycle;
  end
 end
 // Immutable metrics during serialization. 104 little-endian u32 words + CRC32.
 reg [6:0] report_word=0;
 reg [1:0] report_lane=0;
 reg [31:0] selected_word;
 integer client,field;
 always @*begin
  selected_word=0;client=0;field=0;
  case(report_word)
   0:selected_word=32'h434d4453;
   1:selected_word={15'd0,failed,4'd0,mode,CHAIN_GROUP_FOUR?8'd2:8'd1};
   2:selected_word=54000000;
   3:selected_word=WINDOW;
   4:selected_word=measured_elapsed;
   5:selected_word=busy_cycles;
   6:selected_word=read_bytes;
   7:selected_word=write_bytes;
   8:selected_word=groups;
   9:selected_word=group_sum[31:0];
   10:selected_word=0;
   11:selected_word=group_max;
   12:selected_word=missed;
   default:begin
    client=(report_word-13)/13;field=(report_word-13)%13;
    if(client<7)case(field)
     0:selected_word=requests[client];1:selected_word=completed[client];
     2:selected_word=wait_sum[client];3:selected_word=0;
     4:selected_word=first_sum[client];5:selected_word=0;
     6:selected_word=total_sum[client];7:selected_word=0;
     8:selected_word=max_wait[client];9:selected_word=max_first[client];10:selected_word=max_total[client];
     11:selected_word=min_first[client];12:selected_word=min_total[client];
    endcase
   end
  endcase
 end
 reg [31:0] crc=32'hffffffff;
 function [31:0] crc_byte;
  input [31:0] old;
  input [7:0] value;
  reg [31:0] v;integer b;
  begin v=old^{24'd0,value};for(b=0;b<8;b=b+1)v=v[0]?(v>>1)^32'hedb88320:v>>1;crc_byte=v;end
 endfunction
 reg [9:0] uart_shift=10'h3ff;
 reg [3:0] uart_bits=0;
 reg [15:0] uart_timer=0;
 reg [7:0] byte_value;
 always @(posedge clk)begin
  if(reset)begin uart_tx<=1;uart_bits<=0;uart_timer<=0;report_active<=0;report_done<=0;end
  else if(state!=REPORT)begin report_done<=0;report_active<=0;end
  else if(!report_active)begin report_active<=1;report_word<=0;report_lane<=0;crc<=32'hffffffff;end
  else if(uart_bits!=0)begin
   if(uart_timer==0)begin
    uart_tx<=uart_shift[0];uart_shift<={1'b1,uart_shift[9:1]};uart_bits<=uart_bits-1;uart_timer<=UART_DIV-1;
   end else uart_timer<=uart_timer-1;
  end else if(uart_timer!=0)uart_timer<=uart_timer-1;
  else if(report_word==105)report_done<=1;
  else begin
   byte_value=report_word==104?(~crc>>(report_lane*8)):(selected_word>>(report_lane*8));
   if(report_word!=104)crc<=crc_byte(crc,byte_value);
   uart_shift<={1'b1,byte_value,1'b0};uart_bits<=10;uart_timer<=0;
   report_lane<=report_lane+1;
   if(report_lane==3)report_word<=report_word+1;
  end
 end
endmodule
