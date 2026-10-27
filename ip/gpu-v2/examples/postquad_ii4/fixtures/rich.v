module tb;integer unique_i,unique_j;integer stream_fd,matched_line,resident_count,stream_tile;initial stream_fd=$fopen("stream-context.txt","w");integer order_fd;initial order_fd=$fopen("cache-order.txt","w");integer own_dist,own_pub,own_alloc,owned_reads=0,full_edges=0,full_tail_edges=0,unfinished_reads=0,wrap_publications=0,description_checks=0;reg[22:0]canonical_gold[0:31];integer issue_fd,color_fd;initial begin issue_fd=$fopen("issues.txt","w");color_fd=$fopen("color-stream.txt","w");end
 `ifdef GPU_V2_GOWIN_DSP
 GSR GSR(.GSRI(1'b1));
 `endif
 reg clk=0,reset=1,ce=0,abort=0,in_valid=0,draw_rebind=0,flush_request=0;
 reg[22:0]in_begin=0;reg[31:0]in_basic=0;reg[35:0]in_light=0,in_uv=0;
 reg[26:0]draw_config=27'h5600080;reg[23:0]specular=24'h382818;reg[7:0]alpha=128;
 reg[2:0]depth_func=3;reg depth_write=1,blend=1;
 reg[15:0]context_epoch=1;reg[1:0]context_mode=3;reg[4:0]context_code=8;
 reg signed[15:0]light_x=0,light_y=0,light_z=16384,ray_x=-8192,ray_y=-8192,ray_k=8192;
 reg[8:0]ambient=32,directional=192;
 wire in_ready;wire[2:0]in_row;wire flush_complete,pipeline_idle,idle,fault,published;
 wire[4:0]published_slot,work_reserved,work_count;wire[5:0]global_used;
 wire light_result,sample_result,final_issue,final_result,quad_accepted,commit_valid;
 wire[8:0]commit_x;wire[7:0]commit_y;wire[3:0]commit_mask;
 wire mem_req_valid,mem_req_write,mem_write_valid;wire[31:0]mem_req_addr;wire[63:0]mem_write_data;
 reg mem_req_ready=0,mem_read_valid=0,mem_write_ready=0,mem_complete_valid=0,mem_complete_ok=1;reg[63:0]mem_read_data=0;
 gpu_v2_postquad dut(.*);

 task tick;begin clk=1;#2;clk=0;#2;end endtask
 task reset_all;begin reset=1;ce=0;abort=0;#2;tick;reset=0;#2;end endtask
 task no_external;begin
 if(dut.advance!==0||published!==0||final_issue!==0||final_result!==0||quad_accepted!==0||dut.sink.cache.row_accept!==0||dut.sink.cache.depth_fire!==0||dut.sink.cache.leaf_ce!==0||dut.shared_spec_ce!==0||dut.shared_rop_ce!==0)$fatal(1,"fault commit fence");
 end endtask
 initial begin #5000;$fatal(1,"bounded raw-fault watchdog");end
 initial begin
 reset_all;force dut.final_in_valid=1'b1;force dut.final_h=9'd257;
 ce=0;#2;no_external;tick;
 ce=1;#2;if(dut.final_bad!==1)$fatal(1,"Final bad proposal absent");no_external;tick;
 if(fault!==1)$fatal(1,"Final fault not sticky");no_external;
 release dut.final_in_valid;release dut.final_h;
 reset_all;ce=1;force dut.sink.cache.row_valid=1'b1;force dut.sink.cache.row_index=3'd4;
 #2;if(dut.sink_bad!==1)$fatal(1,"sink bad proposal absent");no_external;tick;
 if(fault!==1)$fatal(1,"sink fault not sticky");no_external;
 release dut.sink.cache.row_valid;release dut.sink.cache.row_index;
 reset_all;ce=1;force dut.front.quad.bad_depth=1'b1;#2;no_external;tick;
 if(fault!==1)$fatal(1,"front fault not sticky");release dut.front.quad.bad_depth;
 reset_all;ce=0;force dut.front.backend.member_fault=1'b1;#2;if(fault!==1)$fatal(1,"dut.front.backend.member_fault global visibility");no_external;tick;
 ce=1;#2;no_external;tick;no_external;release dut.front.backend.member_fault;
 reset_all;ce=0;force dut.front.sampling_fault=1'b1;#2;if(fault!==1)$fatal(1,"dut.front.sampling_fault global visibility");no_external;tick;
 ce=1;#2;no_external;tick;no_external;release dut.front.sampling_fault;
 reset_all;ce=0;force dut.front.color_fault=1'b1;#2;if(fault!==1)$fatal(1,"dut.front.color_fault global visibility");no_external;tick;
 ce=1;#2;no_external;tick;no_external;release dut.front.color_fault;
 reset_all;ce=0;force dut.sink_fault=1'b1;#2;if(fault!==1)$fatal(1,"dut.sink_fault global visibility");no_external;tick;
 ce=1;#2;no_external;tick;no_external;release dut.sink_fault;
 reset_all;ce=0;force dut.final_fault=1'b1;#2;if(fault!==1)$fatal(1,"dut.final_fault global visibility");no_external;tick;
 ce=1;#2;no_external;tick;no_external;release dut.final_fault;
 reset_all;ce=1;abort=1;#2;no_external;tick;if(fault!==1)$fatal(1,"abort absent");
 reset_all;ce=1;#2;if(fault!==0||global_used!==0||dut.shade_phase!==0)$fatal(1,"reset discard");
 $display("PASS common-cadence raw Final/sink/front faults, CE0, abort and reset discard");$finish;
 end

reg[255:0]private_control;reg[31:0]private_ids[0:9];reg[37:0]public_adapter;
reg private_hold,public_hold;integer private_i,private_edges=0;
always@(posedge clk)begin
 private_edges=private_edges+1;
 if(!reset)begin
  if(dut.front.light.core.ce && !dut.front.light.core.out_ready)$fatal(1,"private CE implies real ready");
  if((!ce||abort||fault)&&dut.front.light.core.ce)$fatal(1,"private terminal/CE0 fence");
  if(!dut.advance&&(dut.front.light.source_ready||dut.front.light.context_accepted||dut.front.light.pixel_accepted||light_result))$fatal(1,"public acceptance fence");
 end
 private_hold=!reset&&!dut.front.light.core.ce;public_hold=!reset&&!dut.advance;
 private_control={dut.front.light.core.valid_pipe,dut.front.light.core.phase,dut.front.light.core.calendar,dut.front.light.core.configured,dut.front.light.core.ctx_epoch};
 for(private_i=0;private_i<=9;private_i=private_i+1)private_ids[private_i]=dut.front.light.core.id_storage[private_i];
 public_adapter={dut.front.light.normal,dut.front.light.loaded_valid,dut.front.light.loaded_bank};
 #1;
 if(private_hold)begin
  if(private_control!=={dut.front.light.core.valid_pipe,dut.front.light.core.phase,dut.front.light.core.calendar,dut.front.light.core.configured,dut.front.light.core.ctx_epoch})$fatal(1,"private control hold");
  for(private_i=0;private_i<=9;private_i=private_i+1)if(private_ids[private_i]!==dut.front.light.core.id_storage[private_i])$fatal(1,"private token hold");
 end
 if(public_hold&&public_adapter!=={dut.front.light.normal,dut.front.light.loaded_valid,dut.front.light.loaded_bank})$fatal(1,"public adapter hold");
end
final $display("PRIVATE_CONTRACT edges=%0d",private_edges);

endmodule
