"""Compare complete preprocessed factory streams and frozen transport edge traces."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def tokens(source):
    source = re.sub(r'/\*.*?\*/|//[^\n]*', '', source, flags=re.S)
    return re.findall(r'"(?:\\.|[^"\\])*"|[A-Za-z_$][\w$]*|\d+\x27[sS]?[bBdDhHoO][0-9a-fA-F_xXzZ?]+|\d+|\S', source)



def check_command_profile(source, label):
    """Reject legacy storage only in the selected preprocessed command stream."""
    selected = tokens(source)
    for name in ('draw_bank', 'loaded_bank', 'COLOR_BASE', 'DEPTH_BASE', 'draw_rebind', 'L_CONTEXT'):
        assert name not in selected, (label, name)
    for pattern in [('module', 'gpu_vliw_memory', '#'), ('reg', '[', '31', ':', '0', ']', 'far_z')]:
        assert not any(tuple(selected[i:i+len(pattern)]) == pattern for i in range(len(selected))), (label, pattern)


def guard_controls(out):
    # Independent declarations represent the eight original exclusion contracts.
    declarations = [('memory-wrapper', 'module gpu_vliw_memory #(parameter CONTROL=0)();endmodule'),
                    ('draw-bank', 'module probe;wire draw_bank;endmodule'),
                    ('loaded-bank', 'module probe;wire loaded_bank;endmodule'),
                    ('color-base', 'module probe;parameter COLOR_BASE=0;endmodule'),
                    ('depth-base', 'module probe;parameter DEPTH_BASE=0;endmodule'),
                    ('draw-rebind', 'module probe;wire draw_rebind;endmodule'),
                    ('far-z', 'module probe;reg [ 31 : 0 ] far_z;endmodule'),
                    ('lighting-context', 'module probe;localparam L_CONTEXT=0;endmodule')]
    rows = []
    for name, declaration in declarations:
        for monitors in (True, False):
            for active in (True, False):
                source = out/f'guard-{name}-{int(monitors)}-{int(active)}.v'
                source.write_text(declaration if active else '`ifdef RB07_DISABLED_LEGACY\n'+declaration+'\n`endif\nmodule clean;endmodule\n')
                pp = source.with_suffix('.pp')
                z = subprocess.run([os.environ['IVERILOG_EXE'], '-g2012', *([] if monitors else ['-DSYNTHESIS']),
                                    '-E', '-o', str(pp), str(source)], capture_output=True, timeout=30)
                assert z.returncode == 0, z.stderr.decode(errors='replace')
                rejected = False
                try:
                    check_command_profile(pp.read_text(), name)
                except AssertionError:
                    rejected = True
                assert rejected == active, ('ineffective selected-profile guard', name, monitors, active)
                rows.append(dict(guard=name, monitors=monitors, active=active, rejected=rejected))
    return rows


def main():
    p = argparse.ArgumentParser(__doc__)
    p.add_argument('--before', type=Path, required=True)
    p.add_argument('--after', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--traces', action='store_true', help='Require every saved baseline trace to match by relative path')
    a = p.parse_args()
    out = a.output.resolve(); out.mkdir(parents=True, exist_ok=False)
    old, new = a.before.resolve(), a.after.resolve()
    names = {s.name for s in old.glob('*.v')}
    assert names == {s.name for s in new.glob('*.v')}
    assert {'command-native.v', 'command-portable.v', 'generic-memory.v', 'generic-dma.v'} <= names
    rows = []
    for name in sorted(names):
        for monitors in (True, False):
            streams = []
            for side, root in [('before', old), ('after', new)]:
                pp = out/f'{Path(name).stem}-{int(monitors)}-{side}.v'
                z = subprocess.run([os.environ['IVERILOG_EXE'], '-g2012', *([] if monitors else ['-DSYNTHESIS']),
                                    '-E', '-o', str(pp), str(root/name)], capture_output=True, timeout=30)
                assert z.returncode == 0, z.stderr.decode(errors='replace')
                text = pp.read_text(); stream = tokens(text)
                if name.startswith('command-'):
                    check_command_profile(text, (side, name))
                streams.append(stream)
            x, y = streams
            if x != y:
                i = next((i for i, pair in enumerate(zip(x, y)) if pair[0] != pair[1]), min(len(x), len(y)))
                raise AssertionError((name, monitors, i, x[max(0,i-10):i+20], y[max(0,i-10):i+20]))
            rows.append(dict(source=name, monitors=monitors, tokens=len(x),
                             sha256=hashlib.sha256('\n'.join(x).encode()).hexdigest()))
    # Trust is selected only by the private factory scope; no helper can escape it.
    private = ['GPU_V2_COMMAND_TRANSPORT', 'GPU_V2_DMA_ERROR', 'GPU_V2_DMA_BAD', 'GPU_V2_DMA_CLEAR', 'GPU_V2_DMA_FAULT']
    private += ['GPU_V2_ROUTER_'+s for s in ('RO_ERROR','FR_ERROR','FW_ERROR','FB_LEGAL','RO_BAD','FB_BAD','READ_WINDOW','CLEAR','FAULT','RESET_LEGAL','LEGAL','RO_FRAMING','FB_FRAMING')]
    for name in ('command-native.v', 'command-portable.v', 'generic-memory.v', 'generic-dma.v'):
        probe = out/f'{name}-scope.v'
        probe.write_text((new/name).read_text()+''.join(f'\n`ifdef {macro}\nPROFILE_LEAK_{macro}\n`endif\n' for macro in private))
        pp = out/f'{name}-scope.pp.v'
        subprocess.run([os.environ['IVERILOG_EXE'], '-g2012', '-E', '-o', str(pp), str(probe)], check=True, capture_output=True, timeout=30)
        assert 'PROFILE_LEAK_' not in pp.read_text()
        if name.startswith('generic-'):
            probe.write_text('`define GPU_V2_COMMAND_PORTS\n'+(new/name).read_text())
            subprocess.run([os.environ['IVERILOG_EXE'], '-g2012', '-E', '-o', str(pp), str(probe)], check=True, capture_output=True, timeout=30)
            plain = out/f'{Path(name).stem}-1-after.v'
            assert tokens(pp.read_text()) == tokens(plain.read_text()), 'public ports selected trusted transport'
    traces = []
    if a.traces:
        for before in sorted(old.rglob('*.trace')):
            relative = before.relative_to(old)
            # Only completed baseline qualifications supply reference traces.
            receipt = before.parent/'receipt.json'
            if not receipt.is_file() or json.loads(receipt.read_text()).get('status') != 'pass':
                continue
            after = new/relative
            assert after.is_file(), ('missing candidate trace', str(relative))
            x, y = before.read_bytes(), after.read_bytes()
            assert x == y, ('transport edge trace changed', str(relative))
            traces.append(dict(path=str(relative), edges=len(x.splitlines()), sha256=hashlib.sha256(x).hexdigest()))
        assert traces, 'no saved traces'
    controls = guard_controls(out)
    receipt = dict(status='pass', streams=rows, scope_checks=4, traces=traces, guard_controls=controls)
    (out/'receipt.json').write_text(json.dumps(receipt, indent=2))
    print(json.dumps(dict(status='pass', streams=len(rows), guard_controls=len(controls), traces=len(traces), edges=sum(t['edges'] for t in traces))))


if __name__ == '__main__':
    main()
