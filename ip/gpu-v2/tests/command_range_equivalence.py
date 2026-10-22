"""Check actual CP guard wires against the original full-width integer rules."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[3]
SOURCE = ROOT / 'ip/gpu-v2/src/system/command_processor.v'

TB = r'''
`timescale 1ns/1ps
module tb;
reg clk=0,reset=0;
gpu_command_processor dut(.clk(clk),.reset(reset),.ce(1'b0));
integer checks=0,source_index,destination_index,size_index,bit_index,op_index,slot_index;
integer src,dst,size,blocks,record,mesh_blocks,slot,object_blocks;
integer j;
reg[31:0]word;
reg[63:0]seed=64'had1cb27e63f05984;
reg expect_local,expect_mesh,expect_object;
task check;
 input[31:0]value;
 begin
  dut.command=value;
  #1;
  src=(value>>10)&18'h3ffff;dst=(value>>3)&127;size=(value>>1)&3;
  blocks=size==2?4:size==1?2:1;
  expect_local=!value[0]&&size!=3&&(src&(blocks-1))==0&&src+blocks<=262144&&
   ((dst>=66&&dst+blocks<=80)||((dst>>4)==5&&(dst&3)==0&&size==2));
  record=(value>>12)&65535;mesh_blocks=(value>>1)&7;
  expect_mesh=((value>>4)&255)==0&&record+mesh_blocks+2<=65536;
  slot=value&127;object_blocks=value[31:28]==5?2:1;
  expect_object=((value>>7)&21'h1fffff)==0&&slot>=66&&slot+object_blocks<=80;
  if(dut.local_legal!==expect_local||dut.mesh_legal!==expect_mesh||dut.object_legal!==expect_object)
   $fatal(1,"guard mismatch command=%h",value);
  checks=checks+1;
  if(checks>500000)$fatal(1,"vector watchdog");
 end
endtask
function[31:0]random_word;
 begin
  seed=seed^(seed<<13);seed=seed^(seed>>7);seed=seed^(seed<<17);
  random_word=seed[31:0];
 end
endfunction
initial begin
 // All destination/size/low-bit combinations at source wrap and ordinary
 // boundaries. Keep reserved high opcodes varied without changing fields.
 for(source_index=0;source_index<20;source_index=source_index+1)begin
  src=source_index<10?source_index:262144-(20-source_index);
  for(destination_index=0;destination_index<128;destination_index=destination_index+1)
   for(size_index=0;size_index<4;size_index=size_index+1)
    for(bit_index=0;bit_index<2;bit_index=bit_index+1)begin
     word=32'h10000000|(src<<10)|(destination_index<<3)|(size_index<<1)|bit_index;
     check(word);
    end
 end
 // Meshlet low-nibble carry boundaries; every reserved bit must reject.
 for(source_index=0;source_index<40;source_index=source_index+1)begin
  record=source_index<20?source_index:65536-(40-source_index);
  for(size_index=0;size_index<8;size_index=size_index+1)
   for(bit_index=0;bit_index<2;bit_index=bit_index+1)begin
    word=32'h20000000|(record<<12)|(size_index<<1)|bit_index;
    check(word);
    for(j=4;j<12;j=j+1)check(word|(1<<j));
   end
 end
 // Complete object slot/opcode set; each reserved object bit is tested.
 for(op_index=0;op_index<16;op_index=op_index+1)
  for(slot_index=0;slot_index<128;slot_index=slot_index+1)begin
   word=(op_index<<28)|slot_index;check(word);
   for(j=7;j<28;j=j+1)check(word|(1<<j));
  end
 for(j=0;j<100000;j=j+1)begin
  word=random_word();check(word);
 end
 $display("PASS CP_RANGE vectors=%0d seed=%h",checks,seed);
 $finish;
end
endmodule
'''


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    data = SOURCE.read_bytes(); source_sha = hashlib.sha256(data).hexdigest()
    (out / 'dut.v').write_bytes(data)
    (out / 'tb.v').write_text(TB)
    def run(name, command):
        with (out / (name + '.log')).open('w') as log:
            result = subprocess.run(list(map(str, command)), cwd=out, stdout=log,
                                    stderr=subprocess.STDOUT, timeout=20)
        assert result.returncode == 0, (out / (name + '.log')).read_text(errors='replace')[-1800:]
    run('compile', [os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', out / 'sim.vvp', out / 'dut.v', out / 'tb.v'])
    run('run', [os.environ['VVP_EXE'], out / 'sim.vvp'])
    assert SOURCE.read_bytes() == data, 'CP source changed during equivalence run'
    summary = [line for line in (out / 'run.log').read_text().splitlines() if line.startswith('PASS ')]
    assert len(summary) == 1
    receipt = dict(status='pass', source_sha256=source_sha, test_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                   scope='Actual CP combinational guard wires versus original full-width integer rules; not functional throughput',
                   summary=summary[0])
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(receipt))


if __name__ == '__main__': main()
