"""Freeze final command exports and qualify the CP-only controlled-peer checkpoint.

This fixture is not a complete DRAW, native-memory run, or a pixel oracle.
Every compile/run has its own log and a 30-second process bound.
"""
from pathlib import Path
import hashlib
import json
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(sys.argv[1]).resolve()
REPO = Path(__file__).resolve().parents[3]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def execute(args, directory, name):
    with (directory / (name + '.log')).open('wb') as output:
        result = subprocess.run(list(map(str, args)), cwd=directory,
                                stdout=output, stderr=subprocess.STDOUT, timeout=30)
    return result.returncode, (directory / (name + '.log')).read_text(errors='replace')


def module(source, name):
    found = list(re.finditer(r'\bmodule\s+' + re.escape(name) + r'\s*[(#]', source))
    assert len(found) == 1, (name, len(found))
    start = found[0].start()
    end = source.index('endmodule', start) + len('endmodule')
    return source[start:end]


def freeze():
    manifest = {'scope': 'CP objects; controlled source/DMA/render/flush peers',
                'whole_draw': False, 'native_mc': False, 'profiles': {}}
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO).decode().strip()
    manifest['source_head'] = head
    manifest['source_inputs'] = {}
    files = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard',
        'ip/gpu-v2/src', 'ip/gpu-v2/tests/vliw_cycle_draw.rs', 'ip/gpu-v2/tests/vliw_cycle_draw.py',
        'ip/gpu-v2/tests/support/vliw_draw'], cwd=REPO).decode().splitlines()
    for name in files:
        raw = (REPO / name).read_bytes()
        manifest['source_inputs'][name] = sha(raw)
        frozen = ROOT / 'source-freeze' / name
        frozen.parent.mkdir(parents=True, exist_ok=True)
        frozen.write_bytes(raw)
    for profile in ['portable', 'native']:
        raw = (ROOT / f'command-{profile}.v').read_bytes()
        source = raw.decode().replace('\r\n', '\n')
        modules = re.findall(r'\bmodule\s+(\w+)\s*[(#]', source)
        assert len(modules) == len(set(modules))
        assert 'module gpu_v2_vliw_command(' in source
        assert 'texture_fb400240' in source
        assert 'slot_base_f' not in source
        assert 'Fixed-priority single-outstanding transport.' not in source
        assert 'module gpu_v2_packet_store_unique' in source
        assert 'used<48' in module(source, 'gpu_v2_packet_store_unique')
        assert 'global_used<24&&producer_used<8' in module(source, 'gpu_v2_vliw_quad_pool')
        inventory = {}
        for name in modules:
            body = module(source, name)
            header = body[:body.index(');') + 2]
            inventory[name] = {'sha256': sha(body.encode()), 'ports': header,
                               'rom_init_words': len(re.findall(r'INIT_RAM_[0-9A-Fa-f]+|\bmem\[\d+\]\s*=', body))}
        manifest['profiles'][profile] = {'sha256': sha(raw), 'modules': inventory}
    (ROOT / 'profile.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
    # Both full exports elaborate. Running a complete DRAW remains a later gate.
    primitive = Path(os.environ['GOWIN_HOME']) / 'IDE/simlib/gw2a/prim_sim.v'
    (ROOT / 'primitive.json').write_text(json.dumps({'path': str(primitive), 'sha256': sha(primitive.read_bytes())}))
    for profile in ['portable', 'native']:
        directory = ROOT / ('elaborate-' + profile)
        directory.mkdir()
        (directory / 'top.v').write_text("module profile_elaboration;GSR GSR(1'b1);gpu_v2_vliw_command dut();endmodule\n")
        code, log = execute([os.environ['IVERILOG_EXE'], '-g2012', '-s', 'profile_elaboration',
                             '-o', 'command.vvp', ROOT / f'command-{profile}.v', primitive, 'top.v'], directory, 'compile')
        assert code == 0, log[-4000:]
    return manifest


def replay(name, cp, mutation=None, expected=None):
    directory = ROOT / (name + ('-' + mutation if mutation else ''))
    directory.mkdir()
    schema = [(n, int(w), bool(int(o))) for n, w, o in
              (line.split() for line in (ROOT / 'schema.txt').read_text().splitlines())]
    rows = (ROOT / (name + '.txt')).read_text().splitlines()
    assert 0 < len(rows) <= 100000
    if mutation:
        old, new = {
            'ignore_object_ready': ('APPLY_R:if(cp_ready)', "APPLY_R:if(1'b1)"),
            'wrong_uniform_bank': ("{3'b101,page_uniform[{command[0],1'b0}+:2],4'b1011}", "{3'b101,2'd0,4'b1011}"),
            'early_flush': ('FLUSHING:if(flush_complete)', "FLUSHING:if(1'b1)"),
            'wrong_depth_field': ('depth_func<=cp_q[2:0]', 'depth_func<=cp_q[3:1]'),
        }[mutation]
        assert cp.count(old) == 1, mutation
        cp = cp.replace(old, new)
    ports = ['input clk'] + [f'{"output" if out else "input"} [{w-1}:0] {n}' for n, w, out in schema]
    connections = ['.clk(clk)'] + [f'.{n}({n})' for n, _, _ in schema if not n.startswith('obs_')]
    connections += [f'.{n}(1\'b{v})' for n, v in [('clear_ready', 0), ('clear_complete', 0),
                    ('clear_ok', 1), ('ui_done', 0), ('copy_ready', 0), ('copy_complete', 0)]]
    wrapper = 'module draw_cp(' + ','.join(ports) + ');\ngpu_command_processor dut(' + ','.join(connections) + ');\n'
    for n, _, _ in schema:
        if n.startswith('obs_'):
            register = {'phase': 'state', 'cache_pages': 'cache_pages'}.get(n[4:], n[4:])
            wrapper += f'assign {n}=dut.{register};\n'
    wrapper += 'endmodule\n'
    (directory / 'dut.v').write_text(cp + '\n' + wrapper, encoding='utf-8')
    tb = '`timescale 1ns/1ps\nmodule tb;reg clk=0;always #5 clk=~clk;integer fd,n,k;\n'
    names = []
    checks = ''
    for n, w, out in schema:
        tb += f'{"wire" if out else "reg"}[{w-1}:0] {n};\n'
        if out:
            tb += f'reg[{w-1}:0] e_{n};\n'
            names.append('e_' + n)
            checks += f'if({n}!==e_{n})$fatal(1,"DIFF {n} edge%0d got%h expected%h",n,{n},e_{n});\n'
        else:
            names.append(n)
    tb += 'draw_cp top(.clk(clk),' + ','.join(f'.{n}({n})' for n, _, _ in schema) + ');\n'
    tb += 'initial begin fd=$fopen("vectors.txt","r");if(!fd)$fatal;\n'
    fmt = '%h ' * len(schema)
    tb += f'for(n=0;n<{len(rows)};n=n+1)begin @(negedge clk);k=$fscanf(fd,"{fmt}\\n",' + ','.join(names) + ');\n'
    tb += f'if(k!={len(schema)})$fatal(1,"vectors");#1;if(n>0)begin\n' + checks + 'end @(posedge clk);#1;end\n'
    tb += '$display("DRAW_CP_PASS");$finish;end\n'
    tb += f'initial begin #{len(rows)*10+1000};$fatal(1,"watchdog");end endmodule\n'
    (directory / 'tb.v').write_text(tb)
    (directory / 'vectors.txt').write_text('\n'.join(rows) + '\n')
    code, log = execute([os.environ['IVERILOG_EXE'], '-g2012', '-s', 'tb', '-o', 'test.vvp', 'dut.v', 'tb.v'], directory, 'compile')
    assert code == 0, log[-4000:]
    code, log = execute([os.environ['VVP_EXE'], 'test.vvp'], directory, 'run')
    verdict = {'case': name, 'mutation': mutation, 'monitor': expected,
               'edges': len(rows), 'exit': code, 'compiled': True}
    (directory / 'verdict.json').write_text(json.dumps(verdict, indent=2))
    if expected:
        assert code != 0 and expected in log and 'DIFF' not in log, log[-4000:]
    elif mutation:
        assert code != 0 and 'DIFF' in log, log[-4000:]
    else:
        assert code == 0 and 'DRAW_CP_PASS' in log, log[-4000:]
    print(json.dumps(verdict))


freeze()
cp = module((ROOT / 'command-portable.v').read_text(), 'gpu_command_processor')
for name in ['pot', 'view']:
    replay(name, cp)
for mutant in ['ignore_object_ready', 'wrong_uniform_bank', 'early_flush', 'wrong_depth_field']:
    replay('pot', cp, mutation=mutant)
for name, monitor in [('missing_stored', 'CP target drain contract'), ('global_code', 'CP state data contract'),
                      ('target_alias', 'CP state data contract'), ('view_shape', 'CP state data contract'),
                      ('color_high', 'CP mesh color encoding'), ('bad_local', 'CP local DMA contract'),
                      ('bad_mesh', 'CP mesh DMA contract')]:
    replay(name, cp, expected=monitor)
print('DRAW_CP_CHECKPOINT_PASS (not whole DRAW)')
