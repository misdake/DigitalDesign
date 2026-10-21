"""Observer-only bounded replay of T queue pressure and meshlet completion."""
from pathlib import Path
import hashlib
import json
import re
import sys

import replay_export as replay


def observe(bench):
    export = Path(sys.argv[sys.argv.index('--export') + 1]).read_text()
    bits = re.search(r'parameter TRIANGLE_SLOT_BITS=(\d+)', export)
    depth = 1 << int(bits[1]) if bits else 2
    core = 'dut.core.front.meshlet.core'
    meshlet = 'dut.core.front.meshlet'
    declarations = f'''
integer tq_peak=0,tq_full_wait=0,tq_begin=0,tq_last_t=0;
always @(posedge clk)if(!reset&&ce)begin
 if(start&&start_ready)begin
  tq_peak=0;tq_full_wait=0;tq_begin=cycle;tq_last_t=cycle;
 end
 if({core}.count>tq_peak)tq_peak={core}.count;
 if({meshlet}.state=={meshlet}.TSTART&&!{core}.busy&&{core}.count=={depth})
  tq_full_wait=tq_full_wait+1;
 if({meshlet}.state=={meshlet}.TWAIT&&{core}.done_t&&{meshlet}.triangles==1)
  tq_last_t=cycle;
end
'''
    assert bench.count('gpu_v2_vliw_sdram dut(.*);') == 1
    bench = bench.replace('gpu_v2_vliw_sdram dut(.*);',
                          'gpu_v2_vliw_sdram dut(.*);\n' + declarations)
    assert bench.count('$display("MESH task=') == 1
    report = ('$display("TQUEUE task=%0d peak=%0d full_wait=%0d last_t=%0d tail=%0d",'
              'task_id,tq_peak,tq_full_wait,tq_last_t-tq_begin,cycle-tq_last_t);\n ')
    return bench.replace('$display("MESH task=', report + '$display("MESH task=')


def main():
    original_load = replay.load

    def load(name, path):
        module = original_load(name, path)
        if name == 'export_replay_bunny':
            original_bench = module.bench
            module.bench = lambda *a, **kw: observe(original_bench(*a, **kw))
        return module

    replay.load = load
    replay.main()
    out = Path(sys.argv[sys.argv.index('--output') + 1]).resolve()
    rows = [dict((key, int(value)) for key, value in re.findall(r'(\w+)=(\d+)', line))
            for line in (out / 'run.log').read_text().splitlines()
            if line.startswith('TQUEUE ')]
    assert rows
    result = dict(rows=rows, observer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  scope='Per-meshlet enabled-edge TSTART full waits; other times are wall cycles.')
    (out / 'triangle-queue-profile.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result))


if __name__ == '__main__':
    main()
