"""Bounded monitor probes using the existing portable 32-tap IODELAY model."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True
from cl3_pins import with_board_pins

TB = r"""`timescale 1ns/1ps
module Probe(input wire[31:0] pad);
 reg initialized=0,capture_valid=0,dq_drive=0;
 reg[31:0] dq_out=0;
 wire[31:0] dq_sample;
 genvar bit_index;
 generate for(bit_index=0;bit_index<32;bit_index=bit_index+1)begin: delays
  IODELAY #(.C_STATIC_DLY(32)) cell_input(.DI(pad[bit_index]),.DO(dq_sample[bit_index]),
   .DF(),.SDTAP(1'b0),.SETN(1'b1),.VALUE(1'b0));
 end endgenerate
endmodule
module LabPinModel
 #(parameter RETURN_MIN=1,RETURN_MAX=4)(inout wire[31:0] dq);
 wire writing_bus=0;
endmodule
module tb;
 reg controller_clk=0,reset=0;
 always #4.629629 controller_clk=~controller_clk;
 wire O_sdram_clk=0,O_sdram_cke=0,O_sdram_cs_n=1,O_sdram_ras_n=1,O_sdram_cas_n=1,O_sdram_wen_n=1;
 wire[3:0] O_sdram_dqm=15;
 wire[10:0] O_sdram_addr=0;
 wire[1:0] O_sdram_ba=0;
 wire[31:0] IO_sdram_dq;
 reg[31:0] raw=32'h12345678;
 assign IO_sdram_dq=raw;
 Probe dut(IO_sdram_dq);
 LabPinModel #(.RETURN_MIN(1),.RETURN_MAX(4)) pin(.dq(IO_sdram_dq));
 integer scenario,captures=0;
 always @(posedge controller_clk)if(dut.capture_valid)captures=captures+1;
 initial begin
  if(!$value$plusargs("CASE=%d",scenario))$fatal(1,"missing case");
  @(negedge controller_clk);dut.initialized=1;dut.capture_valid=1;
  // Next capture is 4.630 ns away. The existing model delays DQ by 0.800 ns.
  case(scenario)
   0: begin #4.610;raw='hx;#0.040;raw=32'h12345678;end // raw poison spans capture; delayed input stays known
   1: begin #4.635;raw=32'h55555555;end // raw hold transition; consumed transition is 0.805 ns later
   2: begin #3.810;raw=32'h55555555;end // consumed setup is only 0.020 ns
   3: begin #3.835;raw=32'h55555555;end // consumed hold is only 0.005 ns
   4: begin #3.500;raw='hx;#0.900;raw=32'h12345678;end // raw recovers before capture; consumed input is X
   default:$fatal(1,"unknown case");
  endcase
  #2;if(captures!=1)$fatal(1,"capture coverage");
  $display("PROBE_PASS case=%0d captures=%0d",scenario,captures);$finish;
 end
 initial begin #100;$fatal(1,"monitor probe watchdog");end
endmodule
"""


def run(output):
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    source = output / 'tb.v'
    source.write_text(with_board_pins(TB, 'dut'))
    model = Path(__file__).resolve().parents[1] / 'src/sdram_memory_controller/rtl/input_delay_model.v'
    compiled = output / 'monitor.vvp'
    compile_result = subprocess.run([os.environ.get('IVERILOG_EXE', 'iverilog'), '-g2012',
        '-DSTREAM_STATIC_INPUT_DELAY', '-s', 'tb', '-o', str(compiled), str(source), str(model)],
        capture_output=True, timeout=10)
    (output / 'compile.log').write_bytes(compile_result.stdout + compile_result.stderr)
    if compile_result.returncode:
        raise RuntimeError(compile_result.stderr.decode(errors='replace'))
    cases = []
    for index, expected in enumerate([None, None, 'setup', 'hold', 'setup']):
        result = subprocess.run([os.environ.get('VVP_EXE', 'vvp'), str(compiled), f'+CASE={index}'],
                                capture_output=True, timeout=10)
        log = result.stdout + result.stderr
        (output / f'case-{index}.log').write_bytes(log)
        passed = (result.returncode == 0 and b'PROBE_PASS' in log) if expected is None else (
            result.returncode != 0 and f'physical read input {expected}'.encode() in log)
        cases.append(dict(case=index, expected_failure=expected, passed=passed, exit_code=result.returncode))
    receipt = dict(complete=all(c['passed'] for c in cases), cases=cases,
                   simulation_watchdog_ns=100, process_timeout_seconds=10,
                   scope='Monitor observation only; portable IODELAY, not controller/SDRAM/SDF/PVT qualification')
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt))
    if not receipt['complete']:
        raise RuntimeError('Read monitor probes did not match expected results')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--output', required=True, type=Path)
    run(parser.parse_args().output)
