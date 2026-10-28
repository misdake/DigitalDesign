"""Independent packet/address and native/portable cache tests for command16x8."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import random
import re
import subprocess
import sys

sys.dont_write_bytecode=True


def load(path):
    spec=importlib.util.spec_from_file_location(path.stem,path)
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    return module


def vectors(column_major=False):
    rng=random.Random(1608);rows=[]
    for n in range(11):
        side=1<<n
        prefix=sum(4*max(1,(1<<k)//16)**2 for k in range(n))
        for tap in range(4):
            for boundary in range(4):
                for repetition in range(3):
                    x=(rng.randrange(max(1,side//16))*16+(15 if boundary&1 else 2))%side
                    y=(rng.randrange(max(1,side//8))*8+(7 if boundary&2 else 4))%side
                    mask=rng.randrange(1,16)|(1<<tap)
                    quad,lane=rng.randrange(16),rng.randrange(4)
                    first,last=rng.randrange(2),rng.randrange(2)
                    mip,base=rng.randrange(2),rng.randrange(32700)*2
                    member=rng.getrandbits(36)|(mask<<36)|((x//16)<<40)|((y//8)<<46)
                    member|=((x%16)<<53)|((y%8)<<57)|(n<<64)|(quad<<68)|(lane<<72)|(first<<74)|(last<<75)
                    sx=n<=4 or x%16!=15;sy=n<=3 or y%8!=7
                    xx=(x+(tap&1))%side;yy=(y+(tap>>1))%side
                    tile=((xx//16)*max(1,side//16)+yy//16) if column_major else ((yy//16)*max(1,side//16)+xx//16)
                    half=((base+(prefix if mip else 0))//2+tile*2+(yy%16)//8)&32767
                    coverage=sum(int((tap&1 or sx) if i&1 else (not(tap&1) or sx))*
                                 int((tap>>1 or sy) if i&2 else (not(tap>>1) or sy))<<i for i in range(4))
                    owner=(lane<<6)|(quad<<2)|(int(bool(last) and mask>>(tap+1)==0)<<1)|int(bool(first) and tap==0)
                    result=half|((x%16)<<15)|((y%8)<<19)|(owner<<22)|((n&1)<<30)|(coverage<<31)
                    rows.append((member|(tap<<76)|(base<<78)|(mip<<94),result))
    return rows


def execute(command,directory,label,timeout=60):
    with (directory/(label+'.log')).open('wb') as output:
        result=subprocess.run(list(map(str,command)),cwd=directory,stdout=output,stderr=subprocess.STDOUT,timeout=timeout)
    assert result.returncode==0,(directory/(label+'.log')).read_text(errors='replace')[-2500:]


def value(address):
    return (((address>>1)*0x17e9)^(address>>6)^0xa35c)&65535


def main():
    ap=argparse.ArgumentParser(__doc__);ap.add_argument('--export',type=Path,required=True);ap.add_argument('--output',type=Path,required=True)
    ap.add_argument('--column-major',action='store_true')
    args=ap.parse_args();source=args.export.resolve();out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    here=Path(__file__).resolve().parent
    packet=load(here/'texture_packet.py');cache=load(here/'texture_registered_return.py')
    rows=vectors(args.column_major);packetdir=out/'packet';packetdir.mkdir()
    (packetdir/'stim.hex').write_text(''.join(f'{a:024x}\n' for a,b in rows))
    (packetdir/'expected.hex').write_text(''.join(f'{b:09x}\n' for a,b in rows))
    text=packet.TB.replace('@LAST@',str(len(rows)-1)).replace('@COUNT@',str(len(rows))).replace('cycle>3000','cycle>10000')
    (packetdir/'tb.v').write_text(text)
    execute([os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-s','tb','-o','sim.vvp',source,'tb.v'],packetdir,'compile')
    execute([os.environ['VVP_EXE'],'sim.vvp'],packetdir,'run')
    if args.column_major:
        directory=out/'ui-address';directory.mkdir()
        function=re.search(r' function\[8:0\]column_ui_tile;.*?endfunction',source.read_text(),re.S)[0]
        tb='module tb;\n'+function+'\ninteger i;initial begin for(i=0;i<375;i=i+1)if(column_ui_tile(i)!==((i%25)*16+i/25))$fatal(1,"UI tile%0d",i);$display("PASS COLUMN_UI_MAPPING tiles=375");$finish;end endmodule\n'
        (directory/'tb.v').write_text(tb)
        execute([os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o','sim.vvp','tb.v'],directory,'compile')
        execute([os.environ['VVP_EXE'],'sim.vvp'],directory,'run')
    cases=[]
    for native,saturated in [(0,False),(1,False),(0,True),(1,True)]:
        directory=out/f'cache-{native}-{int(saturated)}';directory.mkdir()
        text=cache.TB.replace('@NATIVE@',str(native))
        a="function[34:0]item;input integer i;reg[15:0]sector;begin sector=16'h80+((i/64)%6)*16;item={4'hf,1'b0,8'd0,i[5:3],i[2:0],sector};end endfunction"
        b="function[34:0]item;input integer i;reg[14:0]line_id;begin line_id=15'h80+((i/128)%6)*8;item={4'hf,1'b0,8'd0,i[6:4],i[3:0],line_id};end endfunction"
        assert a in text;text=text.replace(a,b)
        text=text.replace("wire[15:0]item_sector_next=next_item[15:0]","wire[14:0]item_sector_next=next_item[14:0]")
        text=text.replace('383','1535').replace('384','1536').replace('cycle>30000','cycle>100000')
        text=text.replace(".weight_value({4{9'd64}})",".weight_value({9'd203,9'd151,9'd103,9'd54})")
        text=text.replace('got=got+1;end','got=got+1;$display("OUTPUT_CYCLE %0d %0d",got-1,cycle);end')
        if saturated:
            text=text.replace('ce=cycle%7!=2&&cycle%13!=6;allow=cycle%11!=5;ordy=cycle%19>5;mready=!active&&cycle%5!=1;',
                              'ce=1;allow=1;ordy=1;mready=!active;')
            text=text.replace('mvalid=active&&cycle%3!=1;','mvalid=active;')
        (directory/'tb.v').write_text(text)
        cmd=[os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-s','tb','-o','sim.vvp',source,'tb.v']
        if native:cmd.append(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v')
        execute(cmd,directory,'compile');execute([os.environ['VVP_EXE'],'sim.vvp'],directory,'run')
        log=(directory/'run.log').read_text();records=[s.split() for s in log.splitlines() if s.startswith('VALUE ')]
        assert len(records)==1536
        for i,row in enumerate(records):
            x,y=i%16,(i//16)%8;line=0x80+((i//128)%6)*8
            expected=[None]*4;weights=[None]*4
            for tap in range(4):
                xx,yy=(x+tap%2)%16,(y+tap//2)%8
                bank=(((yy&1)^((xx>>1)&1))<<1)|(xx&1)
                expected[bank]=value(line*256+yy*32+xx*2)
                weights[bank]=[54,103,151,203][tap]
            actual=[int(row[3][(3-bank)*4:(4-bank)*4],16) for bank in range(4)]
            assert actual==expected,(native,i,actual,expected)
            assert int(row[2],16)==sum(weights[bank]<<(28+9*bank) for bank in range(4))
        cycles=[int(s.split()[2]) for s in log.splitlines() if s.startswith('OUTPUT_CYCLE ')]
        intervals=[cycles[i]-cycles[i-1] for i in range(1,len(cycles)) if i%128>2]
        if saturated:assert set(intervals)=={2},set(intervals)
        cases.append(dict(native=native,saturated=saturated,
                          warm_intervals=sorted(set(intervals)),summary=[s for s in log.splitlines() if s.startswith('PASS ')]))
    fifo_cases=[]
    for native in (0,1):
        directory=out/f'fifo-{native}';directory.mkdir()
        text=(here/'fixtures/texture_group_q_tb.v').read_text().replace('expected_next[15:0]','expected_next[14:0]')
        (directory/'tb.v').write_text(text)
        execute([os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__',f'-Ptb.NATIVE={native}',
                 '-s','tb','-o','sim.vvp',source,'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'],directory,'compile')
        execute([os.environ['VVP_EXE'],'sim.vvp'],directory,'run')
        fifo_cases.append(dict(native=native,summary=[s for s in (directory/'run.log').read_text().splitlines() if s.startswith('PASS ')]))
    failures=[]
    # Reject either child terminal without publishing a partially filled line.
    # Existing production diagnostics terminate simulation on the peer error.
    for failed_sector in (1,2):
        directory=out/f'failed-sector-{failed_sector}';directory.mkdir()
        text=(out/'cache-0-0/tb.v').read_text()
        text=text.replace(".mem_resp_ok(1'b1)",f'.mem_resp_ok(!(complete&&reqs=={failed_sector}))')
        text=text.replace('endmodule','integer publications=0;always@(posedge clk)if(!reset&&dut.publish)publications=publications+1;\nfinal begin $display("PARENT publications=%0d",publications);end\nendmodule')
        (directory/'tb.v').write_text(text)
        execute([os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-s','tb','-o','sim.vvp',source,'tb.v'],directory,'compile')
        with (directory/'run.log').open('wb') as output:
            result=subprocess.run([os.environ['VVP_EXE'],'sim.vvp'],cwd=directory,stdout=output,stderr=subprocess.STDOUT,timeout=60)
        log=(directory/'run.log').read_text()
        assert result.returncode!=0 and 'GPU contract: mem_resp_ok' in log and 'PARENT publications=0' in log,log[-1500:]
        failures.append(dict(failed_sector=failed_sector,expected_diagnostic=True,publications=0))
    lookahead_cases=[]
    for native in (0,1):
        for thrash in (0,1):
            directory=out/f'lookahead-{native}-{thrash}';directory.mkdir()
            text=(here/'fixtures/texture_group_lookahead_tb.v').read_text()
            text=text.replace('wire[15:0]next_key','wire[14:0]next_key')
            text=text.replace('reg[15:0]sector','reg[14:0]sector')
            text=text.replace("sector=16'h80+(thrash?id*16:0);item={4'hf,1'b0,id,3'd0,3'd0,sector};",
                              "sector=15'h80+(thrash?id*8:0);item={4'hf,1'b0,id,3'd0,4'd0,sector};")
            text=text.replace("((16'h80+(thrash?misses*16:0))*128)","((16'h80+(thrash?(misses/2)*8:0))*256+(misses%2)*128)")
            text=text.replace('misses!=(thrash?96:1)','misses!=(thrash?192:2)').replace('thrash?20:2','thrash?38:2')
            (directory/'tb.v').write_text(text)
            execute([os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__',f'-Ptb.NATIVE={native}',
                     '-s','tb','-o','sim.vvp',source,'tb.v',Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'],directory,'compile')
            execute([os.environ['VVP_EXE'],'sim.vvp',f'+THRASH={thrash}'],directory,'run')
            lookahead_cases.append(dict(native=native,thrash=thrash,summary=[s for s in (directory/'run.log').read_text().splitlines() if s.startswith('PASS ')]))
    abort_cases=[]
    for stage in (0,1,2):
        directory=out/f'abort-{stage}';directory.mkdir()
        text=(out/'cache-0-0/tb.v').read_text()
        text=text.replace(".abort(1'b0)",'.abort(aborted)')
        text=text.replace('reg active=0;','reg aborted=0;reg active=0;')
        text=text.replace('if(bad||fault)$fatal','if((bad||fault)&&!aborted)$fatal')
        condition=['reqs==1&&dut.desc_sector&&!dut.desc_presented',
                   'reqs==1&&dut.desc_sector&&dut.desc_presented&&!dut.desc_started',
                   'reqs==2&&active&&beat==5'][stage]
        injected=((' if(reqs==1&&dut.desc_sector&&!aborted)mready=0;\n' if stage==1 else '')+
                  f' if({condition})aborted=1;\n')
        text=text.replace(' valid=!reset',injected+' valid=!reset')
        trailer=f'''integer publications=0;
always@(posedge clk)if(!reset)begin
 if(dut.publish)publications=publications+1;
 if(aborted&&drained&&!active)begin
  if(reqs!={1 if stage==0 else 2}||publications||got)$fatal(1,"abort conservation");
  $display("PASS ABORT stage={stage} requests=%0d publications=%0d outputs=%0d",reqs,publications,got);$finish;
 end
end
'''
        (directory/'tb.v').write_text(text.replace('endmodule',trailer+'endmodule'))
        execute([os.environ['IVERILOG_EXE'],'-g2012','-D__ICARUS__','-s','tb','-o','sim.vvp',source,'tb.v'],directory,'compile')
        execute([os.environ['VVP_EXE'],'sim.vvp'],directory,'run')
        abort_cases.append(dict(stage=stage,summary=[s for s in (directory/'run.log').read_text().splitlines() if s.startswith('PASS ')]))
    receipt=dict(status='pass',column_major=args.column_major,packet_vectors=len(rows),cache_groups_per_mode=1536,cases=cases,
                 fifo_cases=fifo_cases,failed_child_cases=failures,lookahead_cases=lookahead_cases,abort_cases=abort_cases,
                 export_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                 coverage='All levels0..10; full/base mip; X/Y/XY boundaries; all128 local origins; six conflicting lines; four banks; CE/commit/output/refill pauses and held Q')
    (out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(receipt,indent=2))


if __name__=='__main__':main()
