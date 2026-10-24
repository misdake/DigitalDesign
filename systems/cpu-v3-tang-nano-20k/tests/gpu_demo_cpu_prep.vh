// Observe the exact accepted frame until submission; do not render another frame.
// Clock samples are taken after the CPU/stage registers settle at the falling edge.
integer prep_pc[0:851967];
integer prep_state[0:428];
integer prep_hold[0:12],prep_busy[0:12],prep_scan[0:12];
integer prep_reads[0:12],prep_writes[0:12],prep_refills[0:12],prep_writebacks[0:12];
integer prep_maintenance[0:12];
integer prep_i,prep_j,prep_addr;
initial begin
 for(prep_i=0;prep_i<851968;prep_i=prep_i+1)prep_pc[prep_i]=0;
 for(prep_i=0;prep_i<429;prep_i=prep_i+1)prep_state[prep_i]=0;
 for(prep_i=0;prep_i<13;prep_i=prep_i+1)begin
  prep_hold[prep_i]=0;prep_busy[prep_i]=0;prep_scan[prep_i]=0;
  prep_reads[prep_i]=0;prep_writes[prep_i]=0;
  prep_refills[prep_i]=0;prep_writebacks[prep_i]=0;prep_maintenance[prep_i]=0;
 end
end
always @(negedge clk)if(!reset&&stage>=1&&stage<=12)begin
 prep_addr=(cpu.state==0||cpu.state==1)?cpu.pc_register:cpu.instruction_pc;
 prep_pc[stage*65536+prep_addr]=prep_pc[stage*65536+prep_addr]+1;
 if(hold||sweep||pause||!display_started)begin
  prep_hold[stage]=prep_hold[stage]+1;
  prep_state[stage*33+32]=prep_state[stage*33+32]+1;
 end else prep_state[stage*33+cpu.state]=prep_state[stage*33+cpu.state]+1;
 if(busy)prep_busy[stage]=prep_busy[stage]+1;
 if(cache.state==10||cache.state==13||sweep)prep_scan[stage]=prep_scan[stage]+1;
end
// Transfers count actual rising-edge handshakes; boundaries can differ by one
// marker cycle from falling-edge residency, so these are independent counters.
always @(posedge clk)if(!reset&&stage>=1&&stage<=12)begin
 if(dq&&dr)begin
  if(dw)prep_writes[stage]=prep_writes[stage]+1;
  else prep_reads[stage]=prep_reads[stage]+1;
 end
 if(mq&&mready)begin
  if(mw)prep_writebacks[stage]=prep_writebacks[stage]+1;
  else prep_refills[stage]=prep_refills[stage]+1;
 end
 if(wen&&di==0&&ch==1)prep_maintenance[stage]=prep_maintenance[stage]+1;
end
always @(negedge clk)if(!reset&&stage==13)begin
 if(cycle>200000||starts!=1||fault)$fatal(1,"CPU preparation completion");
 $writememh("__PREP_ARENA__",mem,'h490000/8,'h4b0000/8-1);
 for(prep_i=1;prep_i<=12;prep_i=prep_i+1)begin
  $display("PREP_METRICS stage=%0d hold=%0d maintenance_busy=%0d scan=%0d cpu_reads=%0d cpu_writes=%0d refills=%0d writebacks=%0d maintenance=%0d",prep_i,prep_hold[prep_i],prep_busy[prep_i],prep_scan[prep_i],prep_reads[prep_i],prep_writes[prep_i],prep_refills[prep_i],prep_writebacks[prep_i],prep_maintenance[prep_i]);
  for(prep_j=0;prep_j<33;prep_j=prep_j+1)
   if(prep_state[prep_i*33+prep_j]!=0)$display("PREP_STATE stage=%0d state=%0d cycles=%0d",prep_i,prep_j,prep_state[prep_i*33+prep_j]);
  for(prep_j=0;prep_j<65536;prep_j=prep_j+1)
   if(prep_pc[prep_i*65536+prep_j]!=0)$display("PREP_PC stage=%0d pc=%0d cycles=%0d",prep_i,prep_j,prep_pc[prep_i*65536+prep_j]);
 end
 $display("CPU_PREP_PASS cycle=%0d submit=%0d",cycle,submit_cycle);
 $finish;
end
always @(posedge clk)if(cycle>200000)$fatal(1,"CPU preparation watchdog");
