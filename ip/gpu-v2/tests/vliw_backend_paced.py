"""共享池完整后半：冷启动、共同CE、96Q每16CE周期供给与退休窗口。"""
from pathlib import Path
import importlib.util,sys,argparse
ap=argparse.ArgumentParser(add_help=False);ap.add_argument("--different-screen-quads",action="store_true");extra,rest=ap.parse_known_args();sys.argv=[sys.argv[0]]+rest
p=Path(__file__).with_name('vliw_backend_complete.py');sp=importlib.util.spec_from_file_location('paced_base',p);b=importlib.util.module_from_spec(sp);sys.modules[sp.name]=b;sp.loader.exec_module(b);m=b.m
for opt in ['--steady','--single-texture-tile','--paced']:
 if opt not in sys.argv:sys.argv.append(opt)
if '--count' not in sys.argv:sys.argv+=['--count','96']
t=m.TB.replace('integer front_ticks=','integer admitted=0,ret_pixel=0,geom_cursor=0,lod_cursor=0;integer admit_tick[0:@TOTAL@-1],input_used[0:@TOTAL@-1];integer front_ticks=')
t=t.replace('if(@STEADY@&&wall<700)ce=0;','front_ce=ce;')
a=t.index('  if(start_tick<0');z=t.index('  mem_req_ready=',a)
t=t[:a]+'''  if(cursor<@TOTAL@*8)begin
   cur=rom[cursor];begin_row=cur[130:108];quad_x=begin_row[15:8];quad_y=begin_row[22:16];quad_rgb=begin_row[6];draw_unlit=begin_row[5];draw_bank=begin_row[4];
   quad_valid=phase==0&&(admitted==0||front_ticks>=admit_tick[admitted-1]+16);
  end
  if(ret_pixel<admitted*4)begin
   source_job=ret_pixel/4;source_lane=ret_pixel%4;age=front_ticks-admit_tick[source_job];
   cur=rom[source_job*8];begin_row=cur[130:108];nrow=rom[source_job*8+source_lane*2]>>40;
   tintrow=rom[source_job*8+source_lane*2]>>76;uvrow=rom[source_job*8+source_lane*2+1]>>8;
   if(age>=13+4*source_lane&&age<=16+4*source_lane)begin
    depth_event_pid={source_job[2:0],source_lane[1:0]};depth_event_z16=rom[source_job*8+source_lane*2+1]>>76;depth_event_pass=begin_row[source_lane];depth_event_valid=phase==1;
    result_we=phase!=1;result_wa={source_job[2:0],source_lane[1:0],2'd0};
    if(phase==2)begin result_wa[1:0]=0;result_wd={12'd0,nrow[23:0]};end
    if(phase==3)begin result_wa[1:0]=1;result_wd={6'd0,2'd0,uvrow[15:0],nrow[35:24]};end
    if(phase==0)begin result_wa[1:0]=2;result_wd={2'd0,depth_event_z16,2'd0,uvrow[31:16]};end
   end
  end
  if(geom_cursor<admitted&&front_ticks-admit_tick[geom_cursor]==14)begin
   cur=rom[geom_cursor*8];quad_end_valid=1;quad_end_id=geom_cursor%8;quad_end_mask=cur[111:108];
  end
  if(lod_cursor<admitted&&front_ticks-admit_tick[lod_cursor]==50)begin
   cur=rom[lod_cursor*8];lod_valid=1;lod_id=lod_cursor%8;lod_value=cur[7:0];
  end
'''+t[z:]
t=t.replace('slot_job[(cursor/8)%32]=cursor/8;start_tick=front_ticks;input_ce[cursor/8]=external_ce;', 'slot_job[(cursor/8)%32]=cursor/8;admit_tick[cursor/8]=front_ticks;input_ce[cursor/8]=external_ce;input_used[cursor/8]=global_used;admitted=admitted+1;cursor=cursor+8;')
t=t.replace('   if(start_tick>=0&&front_ticks-start_tick==18)begin cursor=cursor+8;start_tick=-1;end', '''   if(result_we&&result_wa[1:0]==2)ret_pixel=ret_pixel+1;
   if(quad_end_valid)geom_cursor=geom_cursor+1;
   if(lod_valid)lod_cursor=lod_cursor+1;''')
t=t.replace('   for(i=0;i<@BYTES@;', '''   $display("SUSTAIN tail31 inputCE=%0d commitCE=%0d lagStart=%0d lagEnd=%0d usedStart=%0d usedEnd=%0d",input_ce[@WB@]-input_ce[@WA@],commit_ce[@WB@]-commit_ce[@WA@],commit_ce[@WA@]-input_ce[@WA@],commit_ce[@WB@]-input_ce[@WB@],input_used[@WA@],input_used[@WB@]);
   if(input_ce[@WB@]-input_ce[@WA@]!=496||commit_ce[@WB@]-commit_ce[@WA@]!=496||commit_ce[@WA@]-input_ce[@WA@]!=commit_ce[@WB@]-input_ce[@WB@])$fatal(1,"II16 sustained stream/lag failed");
   for(i=0;i<@BYTES@;''')
m.TB=t
s=b.s.replace('if opts.steady:\n        for j,q in enumerate(jobs):','if opts.steady and not opts.paced:\n        for j,q in enumerate(jobs):')
if extra.different_screen_quads:
 s=s.replace('    init=bytearray(',"    for j,q in enumerate(jobs):q['qx']=8+j%8;q['qy']=8+(j//8)%8\n    init=bytearray(")
exec(compile(s,str(p),'exec'),m.__dict__)
if __name__=='__main__':m.main()
