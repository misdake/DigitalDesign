// 本地SP中的32B metadata + 最大1KB V/index；先全部V，再按三个索引一组跑T。
// 外部DMA负责把原始字送入SP；不实现SDRAM或Mesh目录的地址遍历。
module shared_meshlet(
 input clk,reset,ce,start,slot,
`ifdef GPU_V2_COMMAND_PORTS
 input cp_re,input[8:0]cp_addr,output[63:0]cp_q,output cp_ready,
`endif
 input dma_we,input [9:0] dma_addr,input [63:0] dma_data,
 input output_ready,packet_ready,word_ready,
 output reg ready,output busy,drained,output reg done,output reg [3:0] error,
 output dma_ready,output [6:0] published_vertices,
 output packet_valid,output [35:0] descriptor,
 output word_valid,output [4:0] word_index,output [53:0] word_data,
 output emit_valid,output [9:0] emit_addr,output [35:0] emit_data
);
 localparam IDLE=0,META0=1,META0_Q=2,META1=3,META1_Q=4,
            META3=5,META3_Q=6,CHECK=7,VSTART=9,VWAIT=10,
            INDEX=11,INDEX_Q=12,TSTART=13,TWAIT=14;
 reg [3:0] state;
 reg active_slot;
 reg [6:0] vertices,triangles;
 reg [5:0] uniform_base;
 reg [7:0] index_cursor;
 reg [1:0] group_position;
 reg [53:0] index_group;
 reg [3:0] scale,generation;
 reg attr_mode,normal_mode,cull_negative;
 reg [6:0] sequence_id;
 wire core_busy,done_v,done_t,triangle_ready,core_meshlet_ready,source_ready,core_dma_ready;
 wire [2:0] core_error;
 wire [63:0] source_data;
 wire [9:0] vertex_base={2'b00,active_slot,7'd0};
 wire [8:0] config_base={7'd32,active_slot,1'b1}; // 129或131：metadata最后两行。
 wire [35:0] triangle_descriptor={generation,sequence_id,normal_mode,attr_mode,1'b0,scale,index_group[17:0]};
 wire index_read=state==INDEX&&!index_cursor[7];
 wire source_re=state==META0||state==META1||state==META3||index_read;
 wire [1:0] metadata_word=state==META0?2'd0:state==META1?2'd1:2'd3;
 wire [9:0] source_addr=index_read?{2'b00,active_slot,index_cursor[6:0]}:{7'd32,active_slot,metadata_word};
 assign busy=state!=IDLE;
 // 活动输入页和uniform不得被覆盖；另一payload/metadata页仍允许DMA预取。
 wire inactive_payload=dma_addr[9:8]==0&&dma_addr[7]!=active_slot;
 wire inactive_metadata=dma_addr[9:3]==7'd32&&dma_addr[2]!=active_slot;
`ifdef GPU_V2_COMMAND_PORTS
 wire cp_write_region=dma_addr[8:6]==3'b111||
  (dma_addr[8:6]==3'b100&&dma_addr[5:3]!=0);
 wire cp_access=state==IDLE&&!start;
 assign cp_ready=cp_access&&source_ready;
 assign cp_q=source_data;
 wire memory_source_re=source_re||(cp_access&&cp_re);
 wire[9:0]memory_source_addr=cp_access?{1'b0,cp_addr}:source_addr;
`else
 wire cp_write_region=1'b0;
 wire memory_source_re=source_re;
 wire[9:0]memory_source_addr=source_addr;
`endif
 wire write_safe=state==IDLE||inactive_payload||inactive_metadata||cp_write_region;
 assign dma_ready=core_dma_ready&&write_safe;
 wire [8:0] vertex_words={2'd0,vertices}+{1'd0,vertices,1'b0}+9'd1;
 wire [7:0] vertex_rows=vertex_words[8:1];
 wire invalid_header=vertices>64||triangles>96||uniform_base[5:2]!=4'd5||
                     (triangles!=0&&(vertices==0||index_cursor<vertex_rows));
 shared_frontend #(.RESULT_COMPACT(1),.SP_COMPACT(1),.SOURCE_READ(1)) core(
  .clk(clk),.reset(reset),.ce(ce),.start_v(state==VSTART),.start_t(state==TSTART),
  .meshlet_reset(state==CHECK&&!invalid_header),.vertex_count(vertices),.uniform_base(uniform_base),
  .config_base(config_base),.vertex_base(vertex_base),.triangle_descriptor(triangle_descriptor),
  .cull_negative(cull_negative),.dma_we(dma_we&&write_safe),.dma_addr(dma_addr),.dma_data(dma_data),
  .output_ready(output_ready),.packet_ready(packet_ready),.word_ready(word_ready),
  .busy(core_busy),.done_v(done_v),.done_t(done_t),.error(core_error),
  .published_vertices(published_vertices),.packet_valid(packet_valid),.descriptor(descriptor),
  .word_valid(word_valid),.word_index(word_index),.word_data(word_data),
  .dma_ready(core_dma_ready),.emit_valid(emit_valid),.emit_addr(emit_addr),.emit_data(emit_data),
  .source_re(memory_source_re),.source_addr(memory_source_addr),.source_data(source_data),
  .source_ready(source_ready),.triangle_ready(triangle_ready),.meshlet_ready(core_meshlet_ready),.drained(drained)
 );
 always @(posedge clk)begin
  if(reset)begin
   state<=IDLE;ready<=0;done<=0;error<=0;active_slot<=0;vertices<=0;triangles<=0;
   uniform_base<=0;index_cursor<=0;group_position<=0;index_group<=0;
   scale<=0;generation<=0;attr_mode<=0;normal_mode<=0;cull_negative<=0;sequence_id<=0;
  end else if(ce)begin
   done<=0;
   if(state==IDLE&&core_meshlet_ready)ready<=1;
   case(state)
    IDLE:if(start&&ready)begin ready<=0;active_slot<=slot;error<=0;state<=META0;end
    META0:if(source_ready)state<=META0_Q;
    META0_Q:begin
     vertices<=source_data[6:0];triangles<=source_data[14:8];uniform_base<=source_data[21:16];
     index_cursor<={1'b0,source_data[30:24]};cull_negative<=source_data[31];state<=META1;
    end
    META1:if(source_ready)state<=META1_Q;
    META1_Q:begin
     scale<=source_data[3:0];attr_mode<=source_data[4];sequence_id<=source_data[11:5];
     generation<=source_data[15:12];state<=META3;
    end
    META3:if(source_ready)state<=META3_Q;
    META3_Q:begin normal_mode<=source_data[40];state<=CHECK;end
    CHECK:if(invalid_header)begin error<=5;done<=1;state<=IDLE;end else state<=VSTART;
    VSTART:state<=VWAIT;
    VWAIT:if(done_v)begin
     if(core_error!=0)begin error<={1'b0,core_error};done<=1;state<=IDLE;end
     else if(triangles==0)begin done<=1;state<=IDLE;end else state<=INDEX;
    end
    INDEX:if(index_cursor[7])begin error<=6;done<=1;state<=IDLE;end
          else if(source_ready)begin index_cursor<=index_cursor+1'b1;state<=INDEX_Q;end
    INDEX_Q:begin index_group<=source_data[53:0];group_position<=0;state<=TSTART;end
    TSTART:if(triangle_ready)state<=TWAIT;
    TWAIT:if(done_t)begin
     if(core_error!=0)begin error<={1'b0,core_error};done<=1;state<=IDLE;end
     else if(triangles==1)begin triangles<=0;done<=1;state<=IDLE;end
     else begin
      triangles<=triangles-1'b1;sequence_id<=sequence_id+1'b1;
      index_group<={18'd0,index_group[53:18]};
      if(group_position==2)state<=INDEX;
      else begin group_position<=group_position+1'b1;state<=TSTART;end
     end
    end
    default:state<=IDLE;
   endcase
  end
 end
endmodule
