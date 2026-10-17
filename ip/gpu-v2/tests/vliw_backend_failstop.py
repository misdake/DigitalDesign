"""少量闭链非法诊断/外abort资格：局部拒写、故障锁存及已接内存排空。"""
from pathlib import Path
import importlib.util,sys,argparse
ap=argparse.ArgumentParser(add_help=False);ap.add_argument('--fault',choices=['final','texture','abort'],default='final');o,rest=ap.parse_known_args();sys.argv=[sys.argv[0]]+rest
p=Path(__file__).with_name('vliw_backend_complete.py');sp=importlib.util.spec_from_file_location('fault_complete_base',p);b=importlib.util.module_from_spec(sp);sys.modules[sp.name]=b;sp.loader.exec_module(b);m=b.m
t=m.TB.replace('integer front_ticks=','integer injected=0,fault_age=0,failwall=0,held_requests=0;integer front_ticks=')
a=t.index('  if(fault)$fatal');z=t.index('\n',a);t=t[:a]+t[z+1:]
inject={
'final':'''if(dut.final_in_valid&&dut.final_in_ready)begin
 force dut.final_h=9'd257;injected=1;failwall=wall;
 #1;if(!dut.final_bad||dut.final_color.leaf.in_accept)$fatal(1,"local Final invalid-h was not rejected");
end''',
'texture':'''if(mem_read_valid&&pending==1&&!dut.owner&&beat==3)begin
 force dut.txindex=4'd7;injected=1;failwall=wall;
 #1;if(!dut.front.demand.preflight_bad||dut.front.demand.advance)$fatal(1,"local texture bad index was not rejected");
end''',
'abort':'''if(mem_read_valid&&pending==1&&!dut.owner&&beat==3)begin
 abort=1;injected=1;failwall=wall;
 #1;if(!fault||dut.advance||dut.shared_rop_ce||dut.shared_spec_ce)$fatal(1,"external abort did not stop same edge");
end'''}[o.fault]
t=t.replace('  #2;','''  #2;
  if(!injected)begin
'''+inject+'''
  end
  if(fault)begin
   fault_age=fault_age+1;
   if(fault_age==1)held_requests=requests;
   if(dut.advance||dut.shared_rop_ce||dut.shared_spec_ce||dut.front.quad.normal_write||dut.front.quad.uv_write||dut.front.quad.basic_write||dut.front.demand.read_issue||dut.front.color.color.result_write)$fatal(1,"write/compute after failstop");
   if(fault_age>1)begin release dut.final_h;release dut.txindex;abort=0;end
   #1;if(fault_age>1&&!fault)$fatal(1,"fault was not sticky");
  end
  if(dut.front.quad.global_used>24||dut.front.quad.producer_used>8||dut.front.color.color.credits>16||work_reserved>16)$fatal(1,"capacity violated while stopping");
  if(fault_age>80&&pending==0&&dut.arb_state==0)begin
   if(!injected||requests>held_requests+1)$fatal(1,"repeated postfault memory work");
   $display("FAULT drained injected=FAULTKIND detectwall=%0d stopwall=%0d transactions=%0d faultAge=%0d",failwall,wall,requests,fault_age);
   reset=1;ce=0;front_ce=0;abort=0;release dut.final_h;release dut.txindex;#1;clk=1;#2;clk=0;#1;reset=0;#1;
   if(fault||global_used!=0||work_reserved!=0)$fatal(1,"reset did not clear failstop");
   $display("PASS failstop FAULTKIND reset-cleared");$fclose(fd);$finish;
  end
'''.replace('FAULTKIND',o.fault))
m.TB=t
if __name__=='__main__':
    m.main()
    import json
    tag=sys.argv[sys.argv.index('--tag')+1] if '--tag' in sys.argv else 'v2'
    receipt=m.O/f'complete-{tag}-receipt.json';v=json.loads(receipt.read_text())
    v.update(boundary='injected illegal local proposal/texture return/external abort only; local reject, sticky global failstop, no postfault compute/writes, finite accepted memory drain, reset and capacity bounds; does not compare complete final framebuffer after fault',fault_kind=o.fault)
    receipt.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
