// Deliberately conservative: a full pixel-period aperture around each read edge,
// including writes while Q is held. It contains, but does not identify, silicon's
// undocumented asynchronous collision aperture. Exact coincident edges count separately.
localparam real WINDOW=30.031;
realtime last_read=-1.0e12,last_write=-1.0e12;
reg[8:0]last_ra=0,last_rb=0,last_wa=0;
integer audit_read_windows=0,audit_held_windows=0,audit_exact_windows=0;
always @(posedge read_clock)begin
 last_read=$realtime;last_ra=read_address_a;last_rb=read_address_b;
 audit_taint_a<=($realtime-last_write<=WINDOW && last_wa==read_address_a);
 audit_taint_b<=($realtime-last_write<=WINDOW && last_wa==read_address_b);
 if($realtime-last_write<=WINDOW)begin
  if(last_wa==read_address_a)begin audit_read_windows=audit_read_windows+1;if($realtime==last_write)audit_exact_windows=audit_exact_windows+1;end
  if(last_wa==read_address_b)begin audit_read_windows=audit_read_windows+1;if($realtime==last_write)audit_exact_windows=audit_exact_windows+1;end
 end
end
always @(posedge write_clock)if(write_enable)begin
 last_write=$realtime;last_wa=write_address;
 if($realtime-last_read<=WINDOW)begin
  if(write_address==last_ra)begin audit_taint_a<=1;audit_held_windows=audit_held_windows+1;if($realtime==last_read)audit_exact_windows=audit_exact_windows+1;end
  if(write_address==last_rb)begin audit_taint_b<=1;audit_held_windows=audit_held_windows+1;if($realtime==last_read)audit_exact_windows=audit_exact_windows+1;end
 end
end
