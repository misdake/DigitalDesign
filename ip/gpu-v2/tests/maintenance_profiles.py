"""Bounded command-maintenance profile and exact transport-trace checks."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def trace_tb(tb):
    """Observe the actual command, TDP and router edges; never drive the DUT."""
    observer = r'''
integer maintenance_fd,maintenance_mode;
initial begin
 maintenance_mode=0;
 if($value$plusargs("MODE=%d",maintenance_mode))begin end
 maintenance_fd=$fopen($sformatf("maintenance-%0d.trace",maintenance_mode),"w");
end
always @(posedge clk)if(!reset)begin
 $fdisplay(maintenance_fd,"%h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h",
 {ce,abort,busy,fault,dut.ui_active,dut.copy_active,dut.ui_cmd_ready,dut.ui_cmd_done},
 {dut.processor.state,dut.processor.object_index},
 dut.copy_active?{dut.copy_address,dut.cp_q}:78'd0,
 {dut.ui_cmd_valid,dut.ui_cmd_write,dut.ui_lower_ready,dut.ui_tile,dut.ui_pixel_valid,dut.ui_pixel_half,dut.ui_pixel_word},
 dut.ui_pixel_valid?dut.ui_pixels:64'd0,
 {dut.core.back.sink.cache.mt_active,dut.core.back.sink.cache.mt_write,dut.core.back.sink.cache.mt_request,dut.core.back.sink.cache.mt_issue,dut.core.back.sink.cache.mt_data,dut.core.back.sink.cache.mt_retire,dut.core.back.sink.cache.wr_beat_valid},
 {dut.framebuffer_request_valid,dut.framebuffer_request_ready,dut.framebuffer_request_write,dut.framebuffer_read_valid,dut.framebuffer_write_valid,dut.framebuffer_write_ready,dut.framebuffer_complete,dut.framebuffer_ok},
 dut.framebuffer_request_valid?dut.framebuffer_request_address:32'd0,
 dut.framebuffer_read_valid?dut.framebuffer_read_data:64'd0,
 dut.framebuffer_write_valid?dut.framebuffer_write_data:64'd0,
 {fb_r_request_valid,fb_r_request_ready,fb_r_response_valid,fb_r_response_last,fb_r_response_error},
 fb_r_request_valid?{fb_r_address,fb_r_count}:26'd0,
 fb_r_response_valid?fb_r_data:64'd0,
 {fb_w_request_valid,fb_w_request_ready,fb_w_data_ready,fb_w_response_valid,fb_w_response_last,fb_w_response_error},
 fb_w_request_valid?{fb_w_address,fb_w_count}:26'd0,
 fb_w_data_ready?fb_w_data:64'd0,
 {dut.ui_done,dut.compute_idle,dut.memory_busy,dut.dma_idle},
 {dut.core.back.sink.cache.banks[0].a_en,dut.core.back.sink.cache.banks[0].a_we,dut.core.back.sink.cache.banks[0].a_addr},
 {dut.core.back.sink.cache.banks[1].a_en,dut.core.back.sink.cache.banks[1].a_we,dut.core.back.sink.cache.banks[1].a_addr},
 {dut.core.back.sink.cache.banks[2].a_en,dut.core.back.sink.cache.banks[2].a_we,dut.core.back.sink.cache.banks[2].a_addr},
 {dut.core.back.sink.cache.banks[3].a_en,dut.core.back.sink.cache.banks[3].a_we,dut.core.back.sink.cache.banks[3].a_addr},
 {dut.core.back.sink.cache.banks[0].b_en,dut.core.back.sink.cache.banks[0].b_we,dut.core.back.sink.cache.banks[0].b_addr});
end
'''
    assert tb.rstrip().endswith('endmodule')
    return tb.rstrip()[:-len('endmodule')]+observer+'\nendmodule\n'


def tokens(source):
    source=re.sub(r'/\*.*?\*/|//[^\n]*','',source,flags=re.S)
    return re.findall(r'"(?:\\.|[^"\\])*"|[A-Za-z_$][\w$]*|\d+\x27[sS]?[bBdDhHoO][0-9a-fA-F_xXzZ?]+|\d+|\S',source)



DIAGNOSTIC_TB = r"""
`timescale 1ns/1ps
module tb;
reg clk=0;always #5 clk=~clk;
reg reset=1,ce=0,mem_complete_valid=0,mem_complete_ok=1;
reg ui_active=0,ui_lower_ready=1;
wire fault,row_ready,preflight_bad;
GSR GSR(.GSRI(1'b1));
gpu_v2_framebuffer_tdp dut(
 .clk(clk),.reset(reset),.ce(ce),.commit_allow(1'b1),.shade_phase(1'b0),
 .shared_rop_product(54'd0),
 .row_valid(1'b0),.row_tile(9'd0),.row_x(9'd0),.row_y(8'd0),
 .row_mask(4'd0),.row_index(3'd0),.row_data(32'd0),
 .depth_func(3'd7),.depth_write(1'b0),.blend(1'b0),
 .flush_request(1'b0),.prefetch_abort(1'b0),.prefetch_valid(1'b0),.prefetch_tile(10'd0),
 .mem_req_ready(1'b0),.mem_read_data(64'd0),.mem_read_valid(1'b0),
 .mem_write_ready(1'b0),.mem_complete_valid(mem_complete_valid),.mem_complete_ok(mem_complete_ok),
 .fault(fault),.preflight_bad(preflight_bad),.row_ready(row_ready),
`ifdef GPU_V2_COMMAND_PORTS
 .row_alpha(8'd0),
`endif
`ifdef RB03_COMMAND
 .ui_active(ui_active),.ui_cmd_valid(1'b0),.ui_cmd_write(1'b0),.ui_lower_ready(ui_lower_ready),
 .ui_tile(9'd0),.copy_active(1'b0),.copy_address(14'd0),
 .ui_pixel_valid(1'b0),.ui_pixel_half(1'b0),.ui_pixel_word(5'd0),.ui_pixels(64'd0),
 .color_base(14'd2048),.depth_base(14'd2560),
`endif
 .row_accept());
integer cycle=0,kind=0;
always @(posedge clk)begin cycle=cycle+1;if(cycle>25)$fatal(1,"maintenance diagnostic watchdog");end
task tick;begin @(negedge clk);#1;end endtask
initial begin
 if($value$plusargs("KIND=%d",kind))begin end
 repeat(3)tick;reset=0;tick;
 if(kind==0)begin
  // A real failed ACK with a legally owned/complete sector, independently of CE.
  force dut.mt_active=1;force dut.mt_write=0;
  force dut.mt_issue=1;force dut.mt_data=16;force dut.mt_retire=0;
  mem_complete_valid=1;mem_complete_ok=0;
 end else if(kind==1)begin
  // Observe the original diagnostic write site, not another input validator.
  force dut.ingress_error=1'b1;
 end else if(kind==2)begin
  ui_active=1;ui_lower_ready=0;
  force dut.mt_active=1;force dut.mt_write=1;force dut.mt_issue=3;
  force dut.mt_data=48;force dut.mt_retire=2;
 end
 tick;mem_complete_valid=0;
`ifdef RB03_COMMAND
`ifndef SYNTHESIS
 $fatal(1,"missing command maintenance monitor kind=%0d",kind);
`else
 repeat(3)begin tick;if(fault!==0||dut.fault_c!==0||dut.fault_m!==0||dut.source_bad!==0)
  $fatal(1,"command diagnostic feedback appeared during CE0");end
`endif
`else
 if(kind==0&&dut.fault_m!==1)$fatal(1,"generic failed ACK lost its sticky latch");
 if(kind==1&&(dut.fault_c!==1||row_ready!==0))$fatal(1,"generic malformed source was not rejected");
 repeat(3)begin tick;if(fault!==1)$fatal(1,"generic error did not hold during CE0");end
`endif
 $display("PASS MAINTENANCE_DIAGNOSTIC kind=%0d CE0=1",kind);$finish;
end
endmodule
"""


def diagnostics(exports,out):
    tb=out/'diagnostic_tb.v';tb.write_text(DIAGNOSTIC_TB)
    rows=[]
    primitive=Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'
    for backend in ['native','portable']:
        for command,public_ports in [(True,False),(False,False),(False,True)]:
            for synthesis in [False,True]:
                name=f'{backend}-{int(command)}-{int(public_ports)}-{int(synthesis)}'
                source=(exports/f'{"command" if command else "full"}-{backend}.v').resolve()
                executable=out/(name+'.vvp')
                cmd=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-o',executable]
                if command:cmd+=['-DRB03_COMMAND']
                if public_ports:cmd+=['-DGPU_V2_COMMAND_PORTS']
                if synthesis:cmd+=['-DSYNTHESIS']
                result=subprocess.run(list(map(str,[*cmd,source,tb,primitive])),capture_output=True,timeout=45)
                (out/(name+'-compile.log')).write_bytes(result.stdout+result.stderr)
                assert result.returncode==0,result.stderr.decode(errors='replace')[-2000:]
                for kind in ([0,1,2] if command else [0,1]):
                    result=subprocess.run([os.environ['VVP_EXE'],str(executable),f'+KIND={kind}'],capture_output=True,timeout=20)
                    log=result.stdout+result.stderr
                    (out/(name+f'-{kind}.log')).write_bytes(log)
                    if command and not synthesis:
                        expected=[b'GPU contract: mem_complete_ok',b'GPU contract: source_bad',b'UI lower readiness withdrew after publication'][kind]
                        # The same original source predicate also has its write-site monitor.
                        assert result.returncode!=0 and (expected in log or kind==1 and b'GPU contract: fault_c' in log),log.decode(errors='replace')[-2000:]
                    else:
                        assert result.returncode==0 and b'PASS MAINTENANCE_DIAGNOSTIC' in log,log.decode(errors='replace')[-2000:]
                    rows.append(dict(backend=backend,command=command,public_ports=public_ports,synthesis=synthesis,kind=kind))
    return rows

def main():
    parser=argparse.ArgumentParser(__doc__)
    parser.add_argument('--exports',type=Path,required=True)
    parser.add_argument('--baseline',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--diagnostics',action='store_true')
    parser.add_argument('--trace-pair',nargs=2,action='append',type=Path,default=[],metavar=('BASE','CANDIDATE'))
    args=parser.parse_args()
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    rows=[]
    for profile in ['command','full','sdram','backend']:
        for backend in ['native','portable']:
            name=f'{profile}-{backend}'
            for synthesis in [False,True]:
                streams=[]
                for label,directory in [('base',args.baseline),('candidate',args.exports)]:
                    path=(directory/(name+'.v')).resolve()
                    pp=out/f'{label}-{name}-{int(synthesis)}.pp'
                    if profile=='command':
                        probe=out/f'{label}-{name}-scope.v'
                        probe.write_text(path.read_text()+'\n`ifdef GPU_V2_COMMAND_MAINTENANCE\nRB03_PRIVATE_PROFILE_LEAK\n`endif\n')
                        path=probe
                    cmd=[os.environ['IVERILOG_EXE'],'-g2012','-E',*(['-DSYNTHESIS'] if synthesis else []),'-o',pp,path]
                    result=subprocess.run(list(map(str,cmd)),capture_output=True,timeout=60)
                    assert result.returncode==0,result.stderr.decode(errors='replace')[-2000:]
                    text=pp.read_text()
                    assert 'RB03_PRIVATE_PROFILE_LEAK' not in text,'private maintenance profile leaked'
                    selected=tokens(text)
                    if profile=='command':
                        for legacy in ['draw_bank','loaded_bank','COLOR_BASE','DEPTH_BASE','draw_rebind','L_CONTEXT']:
                            assert legacy not in selected, f'inactive generic field reached command RTL: {legacy}'
                        assert not re.search(r'\breg\s*\[\s*31\s*:\s*0\s*\]\s*far_z\b',text), 'retained duplicate command far_z'
                    streams.append(selected)
                if streams[0]!=streams[1]:
                    first=next((i for i,(a,b) in enumerate(zip(*streams)) if a!=b),min(map(len,streams)))
                    raise AssertionError(f'{name} synthesis={synthesis} token={first}: {streams[0][max(0,first-8):first+15]} != {streams[1][max(0,first-8):first+15]}')
                rows.append(dict(profile=name,synthesis=synthesis,tokens=len(streams[0]),sha256=hashlib.sha256('\n'.join(streams[0]).encode()).hexdigest()))
    traces=[]
    for base,candidate in args.trace_pair:
        before=[base] if base.is_file() else sorted(base.glob('maintenance-*.trace'))
        after=[candidate] if candidate.is_file() else sorted(candidate.glob('maintenance-*.trace'))
        assert before and [p.name for p in before]==[p.name for p in after], 'missing trace case'
        for a,b in zip(before,after):
            old,new=a.read_bytes(),b.read_bytes()
            assert old==new, f'per-edge maintenance trace differs: {a} / {b}'
            traces.append(dict(baseline=str(a),candidate=str(b),edges=len(old.splitlines()),sha256=hashlib.sha256(old).hexdigest()))
    negative=diagnostics(args.exports,out) if args.diagnostics else []
    (out/'receipt.json').write_text(json.dumps(dict(status='pass',ordered_complete_tokens=rows,diagnostics=negative,exact_traces=traces),indent=2))
    print(f'PASS {len(rows)} complete ordered preprocessed factory token streams')


if __name__=='__main__':
    main()
