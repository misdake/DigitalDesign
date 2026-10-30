integer way1_stores=0,load_hits=0,tainted_loads=0,wb_checked=0;
reg injected=0;
always @(negedge clk)if($test$plusargs("FORCE_LIVE") && dut.state==dut.ST_LOOKUP && dut.pending_hit && !dut.pending_write && !injected)begin
 if(!dut.hit_way || dut.pending_word[0])$fatal(1,"detector target is no longer the way1 even-bank load");
 force dut.u_data_banks.bank_0_b_read_data=16'hxxxx;injected=1;
end
always @(posedge clk)begin
 if(!reset)begin
  if(dut.hit_store && dut.hit_way)way1_stores=way1_stores+1;
  if(dut.state==dut.ST_LOOKUP && dut.pending_hit && !dut.pending_write)begin
   load_hits=load_hits+1;
   if(dut.hit_way ? (dut.pending_word[0]?dut.u_data_banks.audit_taint_1_b:dut.u_data_banks.audit_taint_0_b) :
                    (dut.pending_word[0]?dut.u_data_banks.audit_taint_1_a:dut.u_data_banks.audit_taint_0_a))tainted_loads=tainted_loads+1;
   if(dut.hit_read_data!==oracle[dut.pending_address])$fatal(1,"AUDIT_LIVE_LOAD addr=%h got=%h expected=%h",dut.pending_address,dut.hit_read_data,oracle[dut.pending_address]);
  end
  if(memory_request_valid && memory_request_ready)begin
   if(memory_address[3:0]!==0 || memory_address>=4096)$fatal(1,"AUDIT_LIVE_WB_ADDRESS addr=%h",memory_address);
  end
  if(write_remaining!=0 && memory_write_data_ready)begin
   if(memory_write_data!=={oracle[transfer_base+4*write_index+3],oracle[transfer_base+4*write_index+2],oracle[transfer_base+4*write_index+1],oracle[transfer_base+4*write_index]})
    $fatal(1,"AUDIT_LIVE_WRITEBACK base=%h beat=%0d got=%h",transfer_base,write_index,memory_write_data);
   wb_checked=wb_checked+1;
  end
 end
end
initial begin
 for(i=0;i<4096;i=i+1)begin memory[i]=16'h8000^i;oracle[i]=16'h8000^i;end
 repeat(2)@(posedge clk);#1 reset=0;
 access(0,32'h20,0,16'h8020);
 access(0,32'h420,0,16'h8420);
 access(1,32'h420,16'hdead,0);
 access(0,32'h420,0,16'hdead);
 access(0,32'h421,0,16'h8421);
 access(1,32'h423,16'hcafe,0);
 access(0,32'h423,0,16'hcafe);
 access(0,32'h422,0,16'h8422);
 access(1,32'h20,16'hbeef,0);
 access(0,32'h820,0,16'h8820);
 access(0,32'hc20,0,16'h8c20);
 access(0,32'h420,0,16'hdead);
 access(0,32'h423,0,16'hcafe);
 maintain(0);
 for(i=0;i<4096;i=i+1)if(memory[i]!==oracle[i])$fatal(1,"AUDIT_LIVE_FINAL_MEMORY address=%h got=%h want=%h",i,memory[i],oracle[i]);
 if(tainted_loads!=0)$fatal(1,"AUDIT_LIVE_TAINT loads=%0d",tainted_loads);
 if(way1_stores!=2 || dut.u_data_banks.audit_collisions<2 || wb_checked<8 || load_hits<4)$fatal(1,"missing positive coverage");
 $display("DIGITAL_DESIGN_PASS DCACHE cycles=%0d way1_stores=%0d load_hits=%0d tainted_effective=%0d collisions=%0d wb_checked=%0d refills=%0d writes=%0d stalled_beats_mask=%h",cycles,way1_stores,load_hits,tainted_loads,dut.u_data_banks.audit_collisions,wb_checked,line_reads,line_writes,stalled_beats);$finish;
end
endmodule
