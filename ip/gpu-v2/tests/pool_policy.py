"""Actual selected Pool ownership, checked policy and DUT mutation qualification."""
import argparse,ast,hashlib,json,os,re,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent

def legacy_tb():
    module=ast.parse((HERE/'vliw_quad_pool.py').read_text())
    return next(ast.literal_eval(n.value) for n in module.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='TB' for t in n.targets))

def adapt(source,rgb):
    body=re.search(r'\bmodule gpu_v2_vliw_quad_pool\b.*?endmodule',source,re.S)[0]
    live='input draw_bank' not in body
    color='quad_color' in body
    tb=legacy_tb().replace('STRESS','1').replace('RESTART','0').replace('NQ','64').replace('RGB',str(rgb))
    if live:
        tb=tb.replace('.draw_bank(bank),','').replace('.light_context_valid(lcontext),','')
        tb=tb.replace('if(!lcontext)$fatal(1,"pixel without stable context");','')
        tb=tb.replace('q[0],mask(q)',"1'b0,mask(q)").replace('lq[0],mask(lq)',"1'b0,mask(lq)").replace('sq[0],mask(sq)',"1'b0,mask(sq)")
    if color:
        tb=tb.replace('.draw_unlit(unlit),',".quad_color(32'h5a445566),.issue_color(),.draw_unlit(unlit),")
        tb=tb.replace('wire[15:0]zd;','wire[23:0]zd;').replace('z_expected,z_held;', 'z_expected;reg[23:0]z_held;')
        tb=tb.replace('if(zd!==z_expected)',"if(zd[15:0]!==z_expected||zd[23:16]!==8'h5a)")
    return tb

OBSERVER=r'''
integer edge_fd;
initial edge_fd=$fopen("edges.trace","w");
always @(posedge clk)if(!reset)$fdisplay(edge_fd,"%h %h %h %h %h %h %h %h %h %h %h %h %h",
 {fc,bc,phase,qv,qr,qid,ev,eid,emask,rw,ra,dv,dpid,lv,lid,pr,pub,ps},
 {retire,rid,skip,empty,skipready,rs,used,prod,lp,sp,busy,fault},
 {br,ba,zr,za,bcol},bd,zd,
 {lvalid,lr,llane,ldesc},lnormal,lposition,
 {svalid,sr,srow,slast,sdesc},sdata,
 {dut.allocated,dut.publish_ptr,dut.retired,dut.complete_tail,dut.light_cursor,dut.sample_cursor},
 {dut.light_state,dut.sample_loaded,dut.attr_pixel,dut.depth_mask},
 {dut.basic_re,dut.basic_write,dut.basic_wa,dut.uv_write,dut.normal_write});
'''

def execute(out,source,tb,native,monitors,expected_failure=False,failure_label=None):
    out.mkdir(parents=True,exist_ok=False);(out/'dut.v').write_text(source);(out/'tb.v').write_text(tb)
    cmd=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb',*(['-DNATIVE'] if native else []),*([] if monitors else ['-DSYNTHESIS']),'-o',str(out/'sim.vvp'),str(out/'dut.v'),str(out/'tb.v')]
    if native:cmd.append(str(Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'))
    z=subprocess.run(cmd,capture_output=True,timeout=45);(out/'compile.log').write_bytes(z.stdout+z.stderr);assert z.returncode==0,z.stderr.decode(errors='replace')[-2000:]
    z=subprocess.run([os.environ['VVP_EXE'],str(out/'sim.vvp')],cwd=out,capture_output=True,timeout=30);log=(z.stdout+z.stderr).decode(errors='replace');(out/'run.log').write_text(log)
    if expected_failure:
        assert failure_label, 'negative case requires its specific rejection'
        assert z.returncode!=0 and 'FATAL' in log and failure_label in log,log[-2000:]
    else:assert z.returncode==0 and 'PASS ' in log,log[-2000:]
    return dict(source_sha256=hashlib.sha256(source.encode()).hexdigest(),tb_sha256=hashlib.sha256(tb.encode()).hexdigest(),summary=log.splitlines()[-2:])

def diagnostic(prefix,command,kind):
    prefix=prefix[:prefix.index('integer reset_count=')]
    return prefix+r'''
integer cycles=0;
always @(posedge clk)begin cycles=cycles+1;if(cycles>30)$fatal(1,"Pool diagnostic watchdog");end
initial begin
 qv=0;rw=0;dv=0;lv=0;ev=0;pr=0;br=0;zr=0;lr=0;sr=0;retire=0;skip=0;fc=0;bc=0;
 repeat(2)@(negedge clk);reset=0;@(negedge clk);
 force dut.allocated=1;force dut.publish_ptr=1;force dut.complete_tail=1;
 force dut.light_cursor=1;force dut.sample_cursor=1;
 bc=1;rid=1;retire=1;
 @UNSAFE@
 @(posedge clk);#2;
 if(fault!==1'b@FAULT@)$fatal(1,"Pool diagnostic feedback policy");
 if(dut.retired!=@RETIRED@)$fatal(1,"Pool diagnostic changed retirement");
 bc=0;retire=0;skip=0;
`ifdef SYNTHESIS
 repeat(3)begin @(posedge clk);#2;if(fault!==1'b@FAULT@)$fatal(1,"Pool CE0 diagnostic lifetime");end
`endif
 $display("PASS Pool diagnostic @KIND@ command=@COMMAND@");$finish;
end
endmodule
'''.replace('@UNSAFE@',"retire=0;skip=1;force dut.head_mask=0;force dut.light_cursor=0;force dut.sample_cursor=0;" if kind else '').replace('@FAULT@',str(int(not command))).replace('@RETIRED@',str(0 if kind else 1)).replace('@KIND@',str(kind)).replace('@COMMAND@',str(int(command)))

def main():
    p=argparse.ArgumentParser(__doc__);p.add_argument('--exports',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--mutations',action='store_true');a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    rows=[];sources={}
    for backend in ['native','portable']:
        for profile in ['backend','command','public-ports','public-live']:
            command=profile=='command'
            name=f'{profile}-{backend}'
            src=(a.exports/(name+'.v')).resolve();pp=out/(name+'.pp.v')
            if profile.startswith('public-'):
                src=out/(name+'.input.v');src.write_text('`define GPU_V2_COMMAND_PORTS\n'+('`define GPU_V2_LIVE_GLOBALS\n' if profile=='public-live' else '')+(a.exports/f'pool-{backend}.v').read_text())
            z=subprocess.run([os.environ['IVERILOG_EXE'],'-g2012','-E','-o',str(pp),str(src)],capture_output=True,timeout=30);assert z.returncode==0,z.stderr.decode(errors='replace')
            source=pp.read_text()
            for monitors in [True,False]:
                z=subprocess.run([os.environ['IVERILOG_EXE'],'-g2012','-E',*([] if monitors else ['-DSYNTHESIS']),'-o',str(pp),str(src)],capture_output=True,timeout=30);assert z.returncode==0
                source=pp.read_text();sources[name+str(monitors)]=source
                for rgb in [0,1]:
                    tb=adapt(source,rgb);tb=tb[:tb.rfind('endmodule')]+OBSERVER+'\nendmodule\n'
                    d=out/f'{name}-{int(monitors)}-rgb{rgb}';r=execute(d,source,tb,backend=='native',monitors);rows.append(dict(case=d.name,**r))
                for kind in [0,1]:
                    d=out/f'{name}-{int(monitors)}-diagnostic{kind}';r=execute(d,source,diagnostic(adapt(source,0),command,kind),backend=='native',monitors,command and monitors,'GPU contract: fault');rows.append(dict(case=d.name,**r))
    mutations=[]
    if a.mutations:
        source=sources['command-portableFalse'];tb=adapt(source,0);tb=tb[:tb.rfind('endmodule')]+OBSERVER+'\nendmodule\n'
        start=source.index('module gpu_v2_vliw_quad_pool(');end=source.index('endmodule',start)
        for name,old,new in [('capacity','global_used<24','global_used<23'),('retire','retired<=retired+1\'b1;','retired<=retired+2;')]:
            body=source[start:end];assert body.count(old)==1,(name,body.count(old));mutant=source[:start]+body.replace(old,new)+source[end:]
            d=out/f'mutant-{name}';r=execute(d,mutant,tb,False,False,True,'stress coverage' if name=='capacity' else 'head descriptor');mutations.append(dict(case=name,**r))
    receipt=dict(status='pass',cases=rows,mutations=mutations,test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest());(out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps(dict(status='pass',cases=len(rows),mutations=len(mutations))))

if __name__=='__main__':main()
