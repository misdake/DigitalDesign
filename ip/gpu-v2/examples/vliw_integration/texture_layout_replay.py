"""Complete recorded demo replay with observer-only texture cache accounting.

Run command-profile RTL against the current physical MC and existing exact
geometry, sampling, lighting, ROP and whole-memory oracle. No hardware access.
"""
from pathlib import Path
import argparse
import copy
import hashlib
import json
import re
import shutil
import struct
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
sys.path.insert(0, str(HERE))
import replay_command as command


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def hierarchy(source, target):
    modules = dict(re.findall(r'\bmodule\s+(\w+)\b(.*?)\bendmodule\b', source, re.S))
    def walk(name, prefix, seen):
        if name == target:
            return prefix
        if name in seen:
            return None
        body = modules[name]
        for child in modules:
            match = re.search(r'\b'+re.escape(child)+r'\s*(?:#\s*\([^;]*?\))?\s+(\w+)\s*\(', body, re.S)
            if match:
                result = walk(child, prefix+'.'+match[1], seen | {name})
                if result:
                    return result
        return None
    result = walk('gpu_v2_vliw_command', 'dut', set())
    assert result, target
    return result


def recording(camera):
    path = ROOT/'systems/cpu-v3-tang-nano-20k/examples/gpu_demo/assets/scene.json'
    scene = json.loads(path.read_text())
    cam = scene['cameras'][camera]
    values = copy.deepcopy(scene['meshes'])
    for i, value in enumerate(values):
        value.update(uniform=cam['matrix']+[65536]+[0]*10, slot=i%2,
                     cull_negative=1, seq_base=0, generation=i%16,
                     cfg_tex_w_log2=6, cfg_tex_h_log2=6, cfg_max_mip=6)
    assert len(values) == 24
    assert sum(len(v['triangles']) for v in values) == 1141
    assert sum(len(v['triangles']) for v in values if v['group']=='bunny') == 948
    shading = dict(light=cam['light'], ambient=64, directional=192,
                   ray_scale=[-8192,-4915], ray_k=8192, shininess=12, specular=[160]*3)
    return dict(instances=values, shading=shading,
                info=dict(scene=scene['scene'], submitted_triangles=1141, meshlets=24,
                          camera_index=camera, source=str(path.relative_to(ROOT)),
                          scene_source_sha256=sha(path)))


def memory16(original, texture):
    memory = original(texture)
    packed = bytearray()
    for n in range(7):
        side = 1 << n
        for ty in range(max(1, side//16)):
            for tx in range(max(1, side//16)):
                for y in range(16):
                    for x in range(16):
                        xx,yy=(tx*16+x)%side,(ty*16+y)%side
                        word = (texture[n][yy*side+xx] if texture is not None else
                                ((xx*3+yy*7+n*5)%32<<11)|((yy*5+xx*11+n*13)%64<<5)|((xx*13+yy*3+n*17)%32))
                        packed.extend(struct.pack('<H',word))
    assert len(packed) == 100*128
    memory[:16384] = packed + bytes(16384-len(packed))
    return memory


def observer(cache, queue):
    expressions = dict(
        line_misses=f'{cache}.tag_write', line_fills=f'{cache}.publish',
        groups=f'{cache}.read_issue', generated_groups=f'{queue}.transfer',
        sector_requests=f'{cache}.mem_req_valid&&{cache}.mem_req_ready',
        return_beats=f'{cache}.mem_resp_valid',
        sector_success=f'{cache}.mem_resp_complete&&{cache}.mem_resp_ok',
        sector_failure=f'{cache}.mem_resp_complete&&!{cache}.mem_resp_ok',
        lookup_cycles=f'{cache}.tag_read',
        cache_wait_cycles=f'{cache}.head_valid&&!{cache}.read_issue',
        refill_wait_cycles=f'{cache}.head_valid&&{cache}.desc_valid',
        tag_wait_cycles=f'{cache}.head_valid&&!{cache}.tag_valid&&!{cache}.desc_valid',
        hit_denial_cycles=f'{cache}.head_valid&&{cache}.hit_ready&&{cache}.tag_valid&&!{cache}.desc_valid&&!{cache}.read_issue',
        ce_denial_cycles=f'{cache}.head_valid&&!{cache}.proposal_ce',
        request_backpressure_cycles=f'{cache}.mem_req_valid&&!{cache}.mem_req_ready',
        fifo_full_cycles=f'{queue}.used==48',
        invalidates='dut.texture_invalidate',
        flushes='ce&&dut.flush_request&&dut.flush_complete')
    text = '\n// Observer only: none of these signals feed the DUT.\n'
    text += ''.join(f'integer tx_{key}=0;\n' for key in expressions)
    text += 'always @(posedge clk) if(!reset&&frame_begin>0&&frame_end==0) begin\n'
    text += ''.join(f' if({value})tx_{key}=tx_{key}+1;\n' for key,value in expressions.items())
    text += 'end\nfinal begin\n $display("TEXTURE '
    text += ' '.join(key+'=%0d' for key in expressions)+'",'
    text += ','.join('tx_'+key for key in expressions)+');\nend\n'
    return text, expressions


def main():
    harness_bytes=Path(__file__).read_bytes()
    harness_hash=hashlib.sha256(harness_bytes).hexdigest()
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--export',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--gold',type=Path,required=True)
    parser.add_argument('--peer',type=Path,required=True)
    parser.add_argument('--layout',choices=('8x8','16x8'),required=True)
    parser.add_argument('--camera-index',type=int,default=0)
    parser.add_argument('--native',action='store_true')
    parser.add_argument('--stress',action='store_true')
    parser.add_argument('--max-cycles',type=int,default=4000000)
    parser.add_argument('--timeout',type=int,default=1800)
    parser.add_argument('--audit-existing',type=Path,help='Re-audit immutable completed traces; require identical DUT, TB, MC and input bytes')
    args = parser.parse_args()
    out = args.output.resolve()
    out.parent.mkdir(parents=True,exist_ok=True)
    recorded = out.parent/(out.name+'-scene.json')
    recorded.write_text(json.dumps(recording(args.camera_index),indent=2))
    source = args.export.read_text()
    cache = hierarchy(source,'gpu_v2_texture_runtime_cache_unique')
    queue = hierarchy(source,'gpu_v2_packet_store_unique')
    counters, definitions = observer(cache,queue)
    original_bench = command.command_bench
    def bench(*values):
        text = original_bench(*values)
        (out/'measurement-harness.py').write_bytes(harness_bytes)
        assert text.count('endmodule')==1
        return text.replace('endmodule',counters+'endmodule')
    command.command_bench = bench
    original_load = command.replay.load
    def load(name,path):
        module = original_load(name,path)
        if name=='export_replay_audit' and args.layout=='16x8':
            original = module.initial_memory
            module.initial_memory = lambda texture=None: memory16(original,texture)
        return module
    command.replay.load = load
    # Cargo test may rebuild the ordinary example path while a long RTL replay
    # runs. Use a content-addressed immutable oracle executable for every audit.
    gold_hash=sha(args.gold)
    frozen_gold=out.parent/'oracles'/gold_hash/args.gold.name
    frozen_gold.parent.mkdir(parents=True,exist_ok=True)
    if not frozen_gold.exists():shutil.copy2(args.gold,frozen_gold)
    assert sha(frozen_gold)==gold_hash
    if args.audit_existing:
        previous=args.audit_existing.resolve()
        def reuse(execute,compile_command,vvp,directory,source,export_sha,timeout):
            command.replay.verify_dut_identity(source,directory/'dut.v',export_sha)
            for path in directory.iterdir():
                if path.suffix in ('.v','.vh') or path.name in ('input.json','seed.txt','jobs.hex','expected-dma.hex','expected-valid.hex','commandbuffer.bin'):
                    assert path.read_bytes()==(previous/path.name).read_bytes(),('re-audit input mismatch',path.name)
            for name in ('front.txt','back.txt','memory.mem','run.log'):
                shutil.copy2(previous/name,directory/name)
            log=(directory/'run.log').read_text()
            assert 'PASS BUNNY ' in log and 'TEXTURE ' in log
            return ''.join(line+'\n' for line in log.splitlines() if line.startswith(('PASS ','ACTIVITY ','WALL '))),0
        command.replay.run_checked_simulation=reuse
    sys.argv = [sys.argv[0], '--export',str(args.export.resolve()), '--output',str(out),
                '--gold',str(frozen_gold), '--peer',str(args.peer.resolve()),
                '--scene','recorded','--recorded',str(recorded),
                '--max-cycles',str(args.max_cycles),'--timeout',str(args.timeout)]
    if args.native:sys.argv.append('--native')
    if args.stress:sys.argv.append('--stress')
    command.main()
    receipt = json.loads((out/'receipt.json').read_text())
    if args.audit_existing:
        receipt.update(simulation_seconds=None,observation_reuse=dict(directory=str(args.audit_existing.resolve()),
                       identity='DUT, observer TB, all MC sources, command bytes and fixture inputs matched byte-for-byte before reusing completed traces. Full numerical audit was rerun with the frozen oracle.'))
        (out/'receipt.json').write_text(json.dumps(receipt,indent=2))
    log = (out/'run.log').read_text()
    line = next(v for v in log.splitlines() if v.startswith('TEXTURE '))
    counts = {key:int(value) for key,value in re.findall(r'(\w+)=(\d+)',line)}
    sectors = 2 if args.layout=='16x8' else 1
    assert counts['line_misses']==counts['line_fills']
    assert counts['sector_requests']==sectors*counts['line_misses']
    assert counts['return_beats']==16*counts['sector_requests']
    assert counts['sector_success']==counts['sector_requests'] and counts['sector_failure']==0
    assert counts['groups']==counts['generated_groups'] and counts['groups']>0
    counts.update(read_bytes=counts['return_beats']*8,
                  miss_rate=counts['line_misses']/counts['groups'],
                  logical_hits=counts['groups']-counts['line_misses'])
    dump = bytes(int(v,16) for v in (out/'memory.mem').read_text().splitlines())
    result = dict(layout=args.layout,data_capacity_bytes=8192,
        resident_lines=32 if sectors==2 else 64,sets=8 if sectors==2 else 16,ways=4,
        counts=counts,definitions=definitions,
        denominator='One access per consumed Group/read_issue; miss counted once at tag_write. Repeated lookup/wait cycles are not accesses.',
        cache_state='Cold after reset and explicit TEX_INVALIDATE; lines retained across all24 meshlets, FB FLUSH at frame end; no per-object texture invalidation.',
        boundary='Complete recorded demo replay, real GPU/MC/pins at frozen TB approximately49.5/99MHz CL2; CPU and concurrent Display excluded. No54/108MHz or board qualification claim.',
        camera_index=args.camera_index,scene_sha256=sha(recorded),
        harness_sha256=harness_hash,commandbuffer_sha256=receipt['commandbuffer_sha256'],
        color_sha256=hashlib.sha256(dump[16384:208384]).hexdigest(),
        depth_sha256=hashlib.sha256(dump[208384:]).hexdigest(),
        dut_sha256=receipt['dut_sha256'],timing=receipt['timing'],activity=receipt['activity'])
    (out/'texture-counts.json').write_text(json.dumps(result,indent=2))
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    main()
