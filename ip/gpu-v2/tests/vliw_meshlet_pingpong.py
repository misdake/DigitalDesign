"""Bounded indexed-attribute lifetime replay across three live meshlets.

Use an explicitly exported current frontend. Arithmetic expectations come from
the independent input oracle, not the candidate's RAM address implementation.
"""
from pathlib import Path
import argparse
import copy
import hashlib
import importlib.util
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[3]
HERE = ROOT / 'ip/gpu-v2/examples/vliw_integration'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


TB = r'''
`timescale 1ns/1ps
module tb;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0;always #5 clk=~clk;
reg reset=1,ce=1,start=0,slot=0,dma_we=0,output_ready=1,packet_ready=0,word_ready=0;
reg[9:0]dma_addr=0;reg[63:0]dma_data=0;
wire ready,busy,done,drained,dma_ready,packet_valid,word_valid,emit_valid;
wire[3:0]error;wire[6:0]published_vertices;wire[31:0]descriptor;wire[35:0]emit_data;
wire[4:0]word_index;wire[53:0]word_data;wire[9:0]emit_addr;
shared_meshlet dut(.*);
localparam FIRST=@FIRST@,TOTAL=FIRST+4,STREAM=@STREAM@;
reg[35:0]expected_v[0:44];reg[31:0]expected_d[0:TOTAL-1];reg[53:0]expected_w[0:TOTAL*16-1];
integer cycle=0,vi=0,pi=0,wi=0,di=0,overlap=0,reuse_blocked=0,paused=0;
integer task_id=0,dm,rc,a,j;reg[63:0]value;reg[1023:0]path;
integer concurrent_attributes=0;
reg held=0;reg[58:0]last_word;reg[31:0]last_desc;reg desc_held=0;
always @(negedge clk)begin
 ce=cycle%7!=2;output_ready=cycle%11!=3;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>12000)$fatal(1,"pingpong watchdog");
 if(!reset)begin
  if(error)$fatal(1,"meshlet error %0d",error);
  if(held&&(!word_valid||{word_index,word_data}!==last_word))$fatal(1,"word changed under hold");
  held=word_valid&&(!ce||!word_ready);last_word={word_index,word_data};
  if(desc_held&&(!packet_valid||descriptor!==last_desc))$fatal(1,"descriptor changed under hold");
  desc_held=packet_valid&&(!ce||!packet_ready);last_desc=descriptor;
  if(!ce)paused=paused+1;
  if(ce)begin
   if(dut.core.vwrite&&dut.core.vfield>=3&&dut.core.export_read&&dut.core.fetch_index>=10)
    concurrent_attributes=concurrent_attributes+1;
   if(emit_valid&&output_ready)begin
    if(emit_addr!==vi%15||emit_data!==expected_v[vi])$fatal(1,"V word %0d",vi);
    vi=vi+1;if(dut.core.count!=0)overlap=overlap+1;
   end
   if(packet_valid&&packet_ready)begin
    if(descriptor!==expected_d[pi])$fatal(1,"descriptor %0d",pi);
    pi=pi+1;
   end
   if(word_valid&&word_ready)begin
    if(word_index!==wi%16||word_data!==expected_w[wi])$fatal(1,"indexed word %0d got%h expected%h",wi,word_data,expected_w[wi]);
    wi=wi+1;
   end
   if(done)di=di+1;
  end
 end
end
task tick;begin @(posedge clk);#1;end endtask
task load_task;input integer n;begin
 task_id=n;slot=n%2;
 $sformat(path,"task%0d/dma.txt",n);dm=$fopen(path,"r");
 while(!$feof(dm))begin
  rc=$fscanf(dm,"%d %h",a,value);
  if(rc==2)begin
   @(negedge clk);dma_addr=a;dma_data=value;dma_we=1;
   tick();while(!dma_ready)tick();@(negedge clk);dma_we=0;
  end
 end
 $fclose(dm);
end endtask
task launch;begin
 while(!ready)tick();@(negedge clk);start=1;
 tick();while(!ce)tick();@(negedge clk);start=0;
end endtask
task completed;input integer n;begin while(di<n)tick();end endtask
initial begin
 $readmemh("vertices.hex",expected_v);$readmemh("descriptors.hex",expected_d);$readmemh("words.hex",expected_w);
 tick();tick();@(negedge clk);reset=0;
 load_task(0);launch();completed(1);
 if(drained||dut.core.count!=FIRST)$fatal(1,"first completion must retain queued T");
 load_task(1);launch();
 if(STREAM)begin @(negedge clk);packet_ready=1;word_ready=1;end
 if(FIRST==16&&!STREAM)begin
  while(vi<30)tick();
  repeat(23)begin tick();if(di!=1||!busy||!dut.core.queue_full)$fatal(1,"full T FIFO did not hold second T after allowing V");end
  @(negedge clk);packet_ready=1;word_ready=1;
 end
 completed(2);
 @(negedge clk);packet_ready=0;word_ready=0;
 if(overlap!=15||dut.core.count<2)$fatal(1,"second V/T did not overlap old T export");
 // Reusing the first attribute epoch is forbidden even though the V/T ALU is idle.
 repeat(23)begin tick();if(ready)$fatal(1,"third meshlet can overwrite first epoch");reuse_blocked=reuse_blocked+1;end
 @(negedge clk);packet_ready=1;word_ready=1;
 // Reuse becomes legal immediately after the first epoch's last indexed word.
 while(wi<FIRST*16)tick();
 // Admission latches availability on the next enabled idle edge. With one
 // isolated CE0 edge every seven clocks, two clocks cover that paid edge.
 tick();tick();if(!ready||drained)$fatal(1,"epoch release did not preserve queued second meshlet");
 @(negedge clk);packet_ready=0;word_ready=0;
 load_task(2);launch();completed(3);
 if(overlap<30)$fatal(1,"third V did not overlap old second meshlet");
 @(negedge clk);packet_ready=1;word_ready=1;
 while(!drained)tick();
 if(vi!=45||pi!=TOTAL||wi!=TOTAL*16||di!=3||paused==0)$fatal(1,"completion counts");
 if(STREAM&&concurrent_attributes==0)$fatal(1,"missing concurrent attribute read/write");
 $display("PASS MESHLET PINGPONG cycles=%0d Vwords=%0d T=%0d words=%0d overlapV=%0d concurrentAttributes=%0d reuseBlocked=%0d CE0=%0d",cycle,vi,pi,wi,overlap,concurrent_attributes,reuse_blocked,paused);
 $finish;
end
initial begin #130000;$fatal(1,"wall timeout");end
endmodule
'''


def main():
    ap = argparse.ArgumentParser(__doc__)
    ap.add_argument('--export', type=Path, required=True)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--native', action='store_true')
    ap.add_argument('--full-queue', action='store_true')
    ap.add_argument('--stream-reads', action='store_true')
    args = ap.parse_args()
    args.full_queue = args.full_queue or args.stream_reads
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    oracle = load('pingpong_oracle', HERE / 'reference/scene/oracle.py')
    oracle = load('pingpong_transport', HERE / 'transport.py').adapt_oracle(oracle)
    source = json.loads((HERE / 'reference/uv-inputs.json').read_text())['scenes'][0]['meshlets'][0]
    vectors, descriptors, words = [], [], []
    for i in range(3):
        instance = copy.deepcopy(source)
        indices = instance['triangles'][0]
        instance['vertices'] = [copy.deepcopy(instance['vertices'][j]) for j in indices]
        triangle_count = 16 if args.full_queue and i == 0 else 2
        instance.update(slot=i % 2, triangles=[[0, 1, 2]]*triangle_count,
                        cull_negative=0, generation=i, seq_base=i*2, color_mode=i % 2)
        for j, v in enumerate(instance['vertices']):
            v[1] = ((40+i*31+j*7) & 1023) | (((-70+i*11-j*5) & 1023) << 10) | ((220+i*17) << 20)
            v[2] = 0x123456 + i*0x231107 + j*0x010203
        meta, packets = oracle.prepare(instance, out / f'task{i}')
        assert meta['vertices'] == 3 and meta['packets'] == triangle_count
        vectors += (out / f'task{i}/vertices.hex').read_text().splitlines()
        descriptors += (out / f'task{i}/descriptors.hex').read_text().splitlines()
        words += (out / f'task{i}/words.hex').read_text().splitlines()
    for name, values in [('vertices', vectors), ('descriptors', descriptors), ('words', words)]:
        (out / (name+'.hex')).write_text('\n'.join(values)+'\n')
    (out / 'tb.v').write_text(TB.replace('@FIRST@', '16' if args.full_queue else '2')
                             .replace('@STREAM@', str(int(args.stream_reads))))
    export = args.export.resolve()
    (out / 'dut.v').write_bytes(export.read_bytes())
    command = [os.environ.get('IVERILOG_EXE', 'iverilog'), '-g2012', '-s', 'tb', '-o', str(out / 'sim.vvp')]
    if args.native:
        command += [str(Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v')]
    else:
        command += ['-DPORTABLE']
    command += [str(out / 'dut.v'), str(out / 'tb.v')]
    for label, cmd in [('compile', command), ('run', [os.environ.get('VVP_EXE', 'vvp'), str(out / 'sim.vvp')])]:
        result = subprocess.run(cmd, cwd=out, capture_output=True, timeout=90)
        (out / (label+'.log')).write_bytes(result.stdout+result.stderr)
        assert result.returncode == 0, (result.stdout+result.stderr).decode(errors='replace')[-3000:]
    receipt = dict(status='pass', native=args.native, full_queue=args.full_queue, stream_reads=args.stream_reads,
                   export_sha256=hashlib.sha256(export.read_bytes()).hexdigest(),
                   log=result.stdout.decode(errors='replace'))
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__':
    main()
