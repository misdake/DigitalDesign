"""只读审计阶段1冻结来源、整数回放、16CE节奏及两布局原始报告。"""
from pathlib import Path
import hashlib
import json
import re

ROOT=Path(__file__).resolve().parents[4]
OUT=ROOT/'target/gpu-vliw-integration-20261018/stage1'
SHA=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()


def read_receipt(name):
    receipt=json.loads((OUT/name).read_text())
    assert receipt['status']=='pass',name
    return receipt


def main():
    frozen=json.loads((OUT/'source-final.json').read_text())
    for rel,h in frozen['files'].items():assert SHA(ROOT/rel)==h,rel
    for rel,h in frozen['receipts'].items():assert SHA(OUT/rel)==h,rel
    functional=read_receipt('complete-tile-final-v1-receipt.json')
    assert len(functional['reports'])==2
    full_replay=[]
    for report in functional['reports']:
        mode=report['mode'];run=OUT/('complete-tile-final-v1-'+mode)
        assert SHA(run/'dut.v')==report['source_sha256']
        assert SHA(run/'trace.txt')==report['trace_sha256']
        assert 'PASS complete sourceQ=9 liveQ=8 pixels=21' in (run/'run.log').read_text()
        assert 'requests=47 CE0paid=33 wall=1453' in report['summary']
        assert (run/'dut.v').read_text()==(OUT/'test-export'/('postquad-'+mode+'.v')).read_text()
        full_replay.append(dict(mode=mode,source_sha256=report['source_sha256'],trace_sha256=report['trace_sha256']))
    assert full_replay[0]['trace_sha256']==full_replay[1]['trace_sha256']
    assert SHA(OUT/'complete-tile-final-v1-inputs.json')==functional['inputs_sha256']
    assert functional['inputs_sha256']==read_receipt('complete-selfcontained-v3-receipt.json')['inputs_sha256']
    assert full_replay[0]['trace_sha256']==read_receipt('complete-selfcontained-v3-receipt.json')['reports'][0]['trace_sha256']

    paced=read_receipt('complete-steady-single-paced96-v1-receipt.json')
    assert SHA(OUT/'complete-steady-single-paced96-v1-inputs.json')==paced['inputs_sha256']
    assert len(paced['reports'])==1 and paced['reports'][0]['mode']=='portable'
    p=paced['reports'][0];run=OUT/'complete-steady-single-paced96-v1-portable'
    assert SHA(run/'dut.v')==p['source_sha256']==full_replay[0]['source_sha256']
    assert SHA(run/'trace.txt')==p['trace_sha256']
    tt=[list(map(int,line.split()[1:])) for line in (run/'trace.txt').read_text().splitlines() if line.startswith('T ')]
    assert [a[0] for a in tt]==list(range(64,96))
    assert tt[-1][2]-tt[0][2]==496
    intervals=[b[2]-a[2] for a,b in zip(tt,tt[1:])]
    assert min(intervals)==15 and max(intervals)==17 and sum(intervals)==496
    domains=dict((k,int(v)) for k,v in re.findall(r'(\w+)=(\d+)',p['summary'].splitlines()[0]))
    assert domains['externalCE']==domains['advanceCE']==domains['coreCE']
    assert domains['inputCE_window']==domains['CissueCE_window']==domains['CcoreCE_window']==496
    assert domains['input_to_commit_start']==domains['end']==83
    assert 'sourceQ=96 liveQ=96 pixels=384 sample=384 light=384' in p['summary']
    inputs=json.loads((OUT/'complete-steady-single-paced96-v1-inputs.json').read_text())
    assert len(inputs['jobs'])==96 and all(q['numerical_mask']==15 and q['textured'] and q['lit'] for q in inputs['jobs'])

    raw=(OUT/'matched-tile-native.v').read_text()
    test=(OUT/'test-export/postquad-native.v').read_text()
    changes=[("DEPTH_BASE=32'd2097152", "DEPTH_BASE=32'd1310720"),
             ("4'd0: slot_base_f = 32'h00000000;", "4'd0: slot_base_f = 32'h00001000;"),
             ("4'd0: slot_max_f = 4'd5;", "4'd0: slot_max_f = 4'd6;")]
    for old,new in changes:
        assert test.count(old)==1
        test=test.replace(old,new,1)
    assert test==raw,'功能与拟合仅允许三处已声明的Slot/Surface常量差异'
    forbidden=['gpu_v2_texture_runtime_d','gpu_v2_texture_serial_lod','gpu_v2_uv_rows',
               'gpu_v2_context_store_shared','gpu_v2_texture_runtime_frontend']
    modules=re.findall(r'(?m)^module\s+(\w+)',raw)
    assert len(modules)==len(set(modules)) and not set(modules).intersection(forbidden)
    assert 'descriptor_tiles[alloc_ptr[3:0]]<=write_tile;' in raw
    assert 'assign descriptor_head_tile=descriptor_tiles[descriptor_head_slot[3:0]];' in raw
    assert raw.count('header_tile<=head_tile;')==2
    assert 'header_tile<=next_tile;' not in raw
    assert 'wire[8:0]write_tile=(write_tile_y << 0)+(write_tile_y << 3)+(write_tile_y << 4)+{4\'d0,in_begin[15:11]};' in raw
    # 完整屏幕坐标只需要375个tile；9bit存储不会溢出。
    for y in range(240):
        for x in range(400):
            begin=(x//2<<8)|(y//2<<16)
            ty=begin>>19&15;tx=begin>>11&31
            tile=ty+(ty<<3)+(ty<<4)+tx
            assert tile==y//16*25+x//16 and 0<=tile<375

    fits=[]
    for place in (0,1):
        directory=OUT/('matched-tile-fit-p'+str(place))
        r=json.loads((directory/'receipt.json').read_text())
        work=directory/('fits/selected-native-p'+str(place)+'-m54')
        for name,key in [('dut.v','source_sha256'),('top.v','wrapper_sha256'),
                         ('impl/pnr/leaf.rpt.txt','report_sha256'),('impl/pnr/leaf.tr','timing_sha256')]:
            assert SHA(work/name)==r[key],(place,name)
        assert r['returncode']==0
        assert raw.replace('module gpu_v2_postquad_direct_lod(','module probe(',1)==(work/'dut.v').read_text()
        rt=(work/'impl/pnr/leaf.rpt.txt').read_text(errors='replace')
        tr=(work/'impl/pnr/leaf.tr').read_text(errors='replace')
        for key,pattern in [('logic',r'^\s*Logic\s*\|\s*(\d+)'),('register',r'^\s*Register\s*\|\s*(\d+)'),
                            ('ssram',r'--SSRAM\(RAM16\)\s*\|\s*(\d+)'),('bsram',r'^\s*BSRAM\s*\|\s*(\d+)')]:
            assert int(re.search(pattern,rt,re.M)[1])==r[key]
        for key,pattern in [('fmax',r'\bclk\s+[\d.]+\(MHz\)\s+([\d.]+)\(MHz\)'),
                            ('setup_violations',r'<Numbers of Setup Violated Endpoints>:(\d+)'),
                            ('hold_violations',r'<Numbers of Hold Violated Endpoints>:(\d+)')]:
            assert float(re.search(pattern,tr)[1])==r[key]
        assert r['fmax']>=54 and r['setup_violations']==r['hold_violations']==0
        assert r['ports']['registered_wrapper_ff']==411
        fits.append(dict(placement=place,receipt_sha256=SHA(directory/'receipt.json'),
                         **{k:r[k] for k in ('logic','register','logic_ff','ssram','bsram','primitives','fmax')}))

    closure=['ip/gpu-v2/src/bin/gpu-v2-vliw-export.rs',
             'ip/gpu-v2/examples/vliw_integration/prepare_uv_inputs.py']
    for rel in closure:
        text=(ROOT/rel).read_text()
        assert 'frontend::vliw' not in text and 'vliw_backend' not in text
    for rel in frozen['files']:
        if rel.endswith('.rs'):
            text=(ROOT/rel).read_text()
            assert 'frontend::vliw' not in text and 'vliw_backend' not in text
    reference=ROOT/'ip/gpu-v2/examples/vliw_integration/reference'
    origin=json.loads((reference/'origin.json').read_text())
    adopted=origin.get('adopted_source_sha256',origin['source_sha256'])
    for rel,h in adopted.items():assert SHA(reference/rel)==h
    whitespace_only=[]
    for rel,h in origin['source_sha256'].items():
        if adopted[rel]!=h:
            original=ROOT/'target/gpu-vliw-integration-20261018/uv-inputs-final/source/reference'/rel
            assert SHA(original)==h
            assert original.read_text().rstrip()==(reference/rel).read_text().rstrip()
            whitespace_only.append(dict(path=rel,upstream_sha256=h,adopted_sha256=adopted[rel]))
    for rel in ['scene/oracle.py','reference.py','math/model.py']:
        text=(reference/rel).read_text()
        assert 'E:/worktrees' not in text and 'E:\\worktrees' not in text
    result=dict(status='pass',auditor_sha256=SHA(__file__),source_final_sha256=SHA(OUT/'source-final.json'),
        frozen_files=frozen['files'],full_integer_replay=full_replay,
        fixed_16CE=dict(quads=96,mode='portable',window_intervals=31,window_CE=496,interval_range=intervals,
                        external_advance_core_equal=True,latency_endpoints_CE=[83,83],scope='指定同纹理tile热负载，16CE输入节奏，无持续队列积压增长'),
        fits=fits,matched_resource_configuration=dict(screen=[400,240],max_n=6,texture_base=0x1000,color_base=0x100000,depth_base=0x140000),
        functional_configuration_differences=changes,wrapper_registers_included=411,old_wrapper_registers=407,
        tile_review='row0且live的真实push同沿写descriptor/descriptor_tiles；同slot/同head地址。完整8行发布后读取，首头与退休换头均在advance采入header_tile；其余沿保持。空mask不分配/不写，旧态16槽信用防同沿回收覆盖。375tile的9bit范围已穷举96000坐标。',
        whitespace_only_adoptions=whitespace_only,
        standalone_dependency_closure=dict(extra_source_sha256={p:SHA(ROOT/p) for p in closure},reference_source_sha256=origin['source_sha256'],adopted_reference_sha256=adopted,
            stage2_required=False,note='只需三个direct_lod模块注册、新Rust/RTL/tests、统一导出CLI、自包含reference五文件及prepare_uv_inputs；不得纳入frontend/mod.rs阶段2增量或CPU/cache dirty。'),
        limitations='完整gold含4个真实VLIW数值quad；Lighting为定向轴法线，depth ALWAYS且单draw。96Q仅portable。资源为完整后半探针非完整GPU；不含CPU/display/物理内存控制器/板测。',
        verdict='阶段1可在上述独立边界提交；阶段2池/前半不是其依赖')
    (OUT/'independent-audit-final.json').write_text(json.dumps(result,ensure_ascii=False,indent=2))
    print(json.dumps(dict(status='pass',fits=fits,output=str(OUT/'independent-audit-final.json')),ensure_ascii=False,indent=2))


if __name__=='__main__':main()
