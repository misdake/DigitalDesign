"""共享权威池→实际完整后半的小型整数回放；不含前半RTL资格。"""
from pathlib import Path
import importlib.util,sys,inspect,hashlib,os
P=Path(__file__).with_name('direct_lod_complete.py')
spec=importlib.util.spec_from_file_location('stage1_frozen_replay',P);m=importlib.util.module_from_spec(spec);sys.modules[spec.name]=m;spec.loader.exec_module(m)
m.O=m.R/'target/gpu-vliw-integration-20261018/stage2';m.O.mkdir(exist_ok=True)
# Replacement hook used by the continuous 16CE producer test.
SERIAL_DRAIN="   if(start_tick>=0&&front_ticks-start_tick==52)begin cursor=cursor+8;start_tick=-1;end"
t=m.TB
old='''reg clk=0,reset=1,ce=0,abort=0,in_valid=0,draw_rebind=0,flush_request=0;
reg[22:0]in_begin=0;reg[31:0]in_basic=0,in_uv=0;reg[35:0]in_light=0;reg[7:0]in_lod=0;'''
new='''reg clk=0,reset=1,ce=0,abort=0,draw_rebind=0,flush_request=0;
reg front_ce=0;reg[1:0]phase=0;reg draw_bank=0,draw_unlit=0;
reg quad_valid=0;wire quad_ready;wire[2:0]quad_id;reg[7:0]quad_x=0;reg[6:0]quad_y=0;reg quad_rgb=0;
reg quad_end_valid=0;reg[2:0]quad_end_id=0;reg[3:0]quad_end_mask=0;
reg result_we=0;reg[6:0]result_wa=0;reg[35:0]result_wd=0;
reg depth_event_valid=0;reg[4:0]depth_event_pid=0;reg[15:0]depth_event_z16=0;reg depth_event_pass=0;
reg lod_valid=0;reg[2:0]lod_id=0;reg[7:0]lod_value=0;wire empty_skipped;wire[5:0]producer_used;
integer front_ticks=0,start_tick=-1,age=-1,source_job=0,source_lane=0,skips=0;
reg[130:0]cur;reg[22:0]begin_row;reg[35:0]nrow;reg[31:0]uvrow,tintrow;'''
assert old in t;t=t.replace(old,new).replace('gpu_v2_postquad_direct_lod dut(.*);','gpu_v2_vliw_backend dut(.*);')
t=t.replace('wire in_ready,','wire ').replace('wire[2:0]in_row;','')
t=t.replace("if({dut.lh,dut.lg}!=={9'd192,9'd224})","if(dut.lg!==9'd224)")
a=t.index('  ce=wall%29!=3');b=t.index('  mem_req_ready=',a)
t=t[:a]+'''  ce=wall%29!=3&&wall%29!=4;front_ce=wall%17!=3;phase=front_ticks%4;
  // Independent producer CE, with a cold long consumer stop to fill the pool.
  if(@STEADY@&&wall<700)ce=0;
  quad_valid=0;quad_end_valid=0;result_we=0;depth_event_valid=0;lod_valid=0;
  if(start_tick<0&&cursor<@TOTAL@*8)begin
   source_job=cursor/8;cur=rom[source_job*8];begin_row=cur[130:108];
   quad_x=begin_row[15:8];quad_y=begin_row[22:16];quad_rgb=begin_row[6];draw_unlit=begin_row[5];draw_bank=begin_row[4];
   quad_valid=phase==0;
  end
  if(start_tick>=0)begin
   age=front_ticks-start_tick;
   source_job=cursor/8;cur=rom[source_job*8];begin_row=cur[130:108];
   if(age>=13&&age<=28)begin
    source_lane=(age-13)/4;nrow=rom[source_job*8+source_lane*2]>>40;
    tintrow=rom[source_job*8+source_lane*2]>>76;uvrow=rom[source_job*8+source_lane*2+1]>>8;
    depth_event_pid={source_job[2:0],source_lane[1:0]};
    depth_event_z16=rom[source_job*8+source_lane*2+1]>>76;
    depth_event_pass=begin_row[source_lane];depth_event_valid=phase==1;
    result_we=phase!=1;result_wa={source_job[2:0],source_lane[1:0],2'd0};
    if(phase==2)begin result_wa[1:0]=0;result_wd={12'd0,nrow[23:0]};end
    if(phase==3)begin result_wa[1:0]=1;result_wd={6'd0,quad_rgb?{10'd0,tintrow[7:0]}:{2'd0,uvrow[15:0]},nrow[35:24]};end
    if(phase==0)begin result_wa[1:0]=2;if(quad_rgb)result_wd={4'd0,depth_event_z16,tintrow[23:8]};else result_wd={2'd0,depth_event_z16,2'd0,uvrow[31:16]};end
   end
   if(age==14)begin quad_end_valid=1;quad_end_id=source_job%8;quad_end_mask=begin_row[3:0];end
   if(age==(quad_rgb?15:50))begin lod_valid=1;lod_id=source_job%8;lod_value=quad_rgb?8'd0:cur[7:0];end
  end
''' +t[b:]
t=t.replace('  if(published)begin slot_job[published_slot]=cursor/8;pubs=pubs+1;end','''  if(front_ce&&quad_valid&&quad_ready)begin
   slot_job[(cursor/8)%32]=cursor/8;start_tick=front_ticks;input_ce[cursor/8]=external_ce;
  end
  if(published)pubs=pubs+1;
  if(empty_skipped)begin skips=skips+1;$fdisplay(fd,"E %0d",dut.retire_slot);end''')
t=t.replace('  if(in_valid&&in_ready)begin if(cursor%8==0)input_ce[cursor/8]=external_ce;cursor=cursor+1;end','')
t=t.replace('if(pubs!=@LIVE@||rets!=@LIVE@','if(pubs!=@TOTAL@||skips!=@TOTAL@-@LIVE@||rets!=@LIVE@')
# Only the old steady diagnostics were keyed to row ingress. New repeated tests
# verify finite credit/wrap and all output values, not an inferred arithmetic II.
a=t.index('   if(@STEADY@)begin');b=t.index('   for(i=0;i<@BYTES@;',a);t=t[:a]+t[b:]
t=t.replace('  clk=1;#2;clk=0;#1;','''  clk=1;#2;clk=0;#1;
  if(front_ce)begin
@SERIAL_DRAIN@
   front_ticks=front_ticks+1;
  end'''.replace('@SERIAL_DRAIN@',SERIAL_DRAIN))
t=t.replace('integer wall=0,','integer peak=0,wraps=0;integer wall=0,')
t=t.replace('integer peak=0,', 'integer source_reads=0,source_skips=0;integer z_reads=0,z_depths=0;reg z_owner=0,z_pending=0,z_keep=0;reg[6:0]z_key;reg[15:0]z_expected,z_held;integer peak=0,')
hold_cycles=int(os.environ.get('VLIW_SOURCE_HOLD_CYCLES','0'));assert 0<=hold_cycles<=200
t=t.replace('integer source_reads=0,', 'integer source_hold_left=0,source_hold_steps=0;reg source_hold_used=0,source_q_hold=0;reg[75:0]source_q;integer source_reads=0,')
t=t.replace('  quad_valid=0;', '  if(source_hold_left>0)begin ce=0;source_hold_left=source_hold_left-1;source_hold_steps=source_hold_steps+1;end\n  quad_valid=0;')
t=t.replace('  if(published)pubs=pubs+1;','  if(global_used>peak)peak=global_used;\n  if(published)begin pubs=pubs+1;if(published_slot==31)wraps=wraps+1;end')
t=t.replace('liveQ=%0d pixels=', 'publishedQ=%0d pixels=')
t=t.replace('   for(i=0;i<@BYTES@;', '   $display("POOL peak=%0d fullslot31=%0d empty_skips=%0d",peak,wraps,skips);\n   for(i=0;i<@BYTES@;')
# Per-wall accepted-event signature for legal old/new structural comparisons.
t=t.replace('  if(published)pubs=pubs+1;', '  if(published)pubs=pubs+1;')
t=t.replace('  if(dut.advance)enabled=enabled+1;', '\n'.join([
 '  if(dut.advance)enabled=enabled+1;',
 '  if(source_q_hold&&{dut.basic_data,dut.light_data,dut.sample_data,dut.input_bypass,dut.issue_slot,dut.issue_lane,dut.state}!==source_q)$fatal(1,"CE0 changed J_WAIT source ownership");',
 '  source_q_hold=!ce&&dut.state==dut.J_WAIT;source_q={dut.basic_data,dut.light_data,dut.sample_data,dut.input_bypass,dut.issue_slot,dut.issue_lane,dut.state};',
 f'  if(dut.advance&&dut.read_values&&!source_hold_used)begin source_hold_used=1;source_hold_left={hold_cycles};end',
 '  if(dut.advance&&dut.read_values)begin if(!dut.covered)$fatal(1,"masked source RAM read");source_reads=source_reads+1;end',
 '  if(dut.advance&&dut.skip_issue)begin',
 '   if(dut.covered||dut.basic_read||dut.state!=dut.J_READ||dut.final_in_valid||!dut.issue_room)$fatal(1,"masked lane did not bypass operands and wait");',
 '   source_skips=source_skips+1;',
 '  end',
 '  if(z_pending&&dut.depth_data!==z_expected)$fatal(1,"C-paid Z return mismatch");',
 '  if(z_keep&&dut.depth_data!==z_held)$fatal(1,"Z Q changed without a C acceptance");',
 '  z_pending=dut.sink_color_accept;z_keep=!dut.sink_color_accept;z_held=dut.depth_data;',
 '  if(dut.sink_color_accept)begin',
 '   if(z_owner||!dut.output_issued)$fatal(1,"C overtook paired D or source issue");',
 '   z_owner=1;z_key={dut.retire_slot,dut.out_lane};z_reads=z_reads+1;',
 '   z_expected=rom[slot_job[dut.retire_slot]*8+dut.out_lane*2+1]>>76;',
 '  end',
 '  if(dut.lane_accept)begin',
 '   if(!z_owner||!dut.color_paid||z_key!=={dut.retire_slot,dut.out_lane}||dut.depth_data!==z_expected)$fatal(1,"D without its C-paid Z Q");',
 '   z_owner=0;z_depths=z_depths+1;',
 '  end',
 '  $fdisplay(fd,"B %0d %h %0d %0d",wall,{ce,front_ce,(front_ce&&quad_valid&&quad_ready),dut.advance,published,dut.front.quad.normal_write,dut.front.quad.uv_write,dut.front.quad.basic_write,(dut.advance&&dut.basic_read),dut.front.light.pixel_accepted,light_result,dut.front.sample_fire,dut.front.sampling.c_issue,dut.front.coefficient_accept,dut.front.backend.member_issue,dut.front.demand.read_issue,dut.front.color.input_fire,dut.public_publish,sample_result,final_issue,final_result,dut.lane_accept,quad_accepted,empty_skipped,commit_valid,(mem_req_valid&&mem_req_ready),mem_read_valid,(mem_write_valid&&mem_write_ready),mem_complete_valid},dut.issue_slot,dut.retire_slot);',
 '  if(dut.front.quad.normal_write)$fdisplay(fd,"N %0d %h %h",wall,dut.front.quad.normal_bank.wa,dut.front.quad.normal_bank.wd);',
 '  if(dut.front.quad.uv_write)$fdisplay(fd,"U %0d %h %h",wall,dut.front.quad.uv_bank.wa,dut.front.quad.uv_bank.wd);',
 '  if(dut.front.quad.basic_write)$fdisplay(fd,"D %0d %h %h",wall,dut.front.quad.basic_bank.wa,dut.front.quad.basic_bank.wd);'
]))
t=t.replace('   for(i=0;i<@BYTES@;', f'   if(source_hold_steps!={hold_cycles})$fatal(1,"source CE0 hold coverage");\n   if(z_owner||z_reads!=@LIVE@*4||z_depths!=z_reads)$fatal(1,"paired Z conservation");\n   if(source_reads!=@PIXELS@||source_skips!=@LIVE@*4-@PIXELS@)$fatal(1,"covered read/masked skip conservation");\n   $display("SOURCE reads=%0d skips=%0d holdCE0=%0d",source_reads,source_skips,source_hold_steps);\n   for(i=0;i<@BYTES@;')
m.TB=t
s=inspect.getsource(m.main)
s=s.replace("ap.add_argument('--tag',default='v2')","ap.add_argument('--tag',default='producer-calendar-v1')")
s=s.replace("shutil.copy2(O/f'test-export/postquad-{mode}.v',d/'dut.v')","shutil.copy2(Path(os.environ.get('VLIW_BACKEND_EXPORT',str(O/'backend-export')))/f'backend-{mode}.v',d/'dut.v')")
s=s.replace('    init=bytearray(',"    if opts.steady:\n        for j,q in enumerate(jobs):\n            q['textured']=j%5!=0;q['lit']=j%3!=0\n            if j%7==0:q['numerical_mask']=0\n            for l in q['lanes']:\n                l['uv_s18f16']=[4352+17*j,4864+13*j];l['depth_u16q8_8']=512+j\n    for q in jobs:\n        if q['textured']:q['tint']=[[255,255,255]]*4\n    init=bytearray(")
s=s.replace("'boundary':'actual VLIW UV/LOD/Z codes on four quads; directed empty/RGB-lit/full cases; selected integer tap/Final/ROP and whole memory oracle; not whole GPU'","'boundary':'shared producer pool plus actual complete selected backend; real VLIW code fixture plus directed axis lighting; independent final/DRAM gold, not full frontend RTL; repeated mode tests finite wrap not arithmetic II'")
exec(compile(s,str(P), 'exec'),m.__dict__)
if __name__=='__main__':m.main()
