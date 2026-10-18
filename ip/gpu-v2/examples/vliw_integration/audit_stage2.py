"""只读复核完整前后半回放的来源、事件、整幅内存与计数口径。"""
from pathlib import Path
import argparse,hashlib,json,re
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[3]
SHA=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()

def top_ports(source):
    head=re.search(r'\bmodule\s+gpu_v2_vliw\s*\((.*?)\);',source,re.S).group(1)
    head=re.sub(r'//[^\n]*|/\*.*?\*/','',head,flags=re.S)
    direction=None;width=1;ports={}
    for group in head.split(','):
        m=re.search(r'\b(input|output|inout)\b',group)
        if m:
            direction=m[1];w=re.search(r'\[(\d+)\s*:\s*(\d+)\]',group)
            width=abs(int(w[1])-int(w[2]))+1 if w else 1
        name=re.search(r'([A-Za-z_]\w*)\s*$',group)[1]
        assert direction and name not in ports
        ports[name]=dict(direction=direction,width=width)
    return ports

def spans(path):
    result={}
    for line in path.read_text().splitlines():
        a=line.split();kind=a[0];cycle=int(a[1]);v=result.setdefault(kind,dict(first=cycle,last=cycle,count=0))
        v['last']=cycle;v['count']+=1
    for v in result.values():v['wall_span']=v['last']-v['first']
    return result

def audit_ndc(source):
    body=re.search(r'\bmodule\s+gpu_v2_vliw_ndc_rom\b(.*?)endmodule',source,re.S)[1]
    def rnd(n,d):q,r=divmod(n,d);return q+int(2*r>d or 2*r==d and q&1)
    want=[]
    for row in range(320):
        if row<200:pair=[rnd((2*(2*row+j)+1-400)*16384,400) for j in (0,1)]
        else:pair=[rnd((240-2*(2*(row-200)+j)-1)*16384,240) for j in (0,1)]
        want.append((pair[0]&65535)|((pair[1]&65535)<<16))
    direct={int(i):int(v,16) for i,v in re.findall(r'memory\[(\d+)\]=32\'h([0-9a-fA-F]+)',body)}
    if direct:assert [direct[i] for i in range(320)]==want
    else:
        init={int(i,16):int(v,16) for i,v in re.findall(r'\.INIT_RAM_([0-9a-fA-F]{2})\(288\'h([0-9a-fA-F]+)\)',body)}
        assert set(init)==set(range(64));got=[(init[i//8]>>(36*(i%8)))&((1<<36)-1) for i in range(512)]
        assert got==want+[0]*192
    return dict(entries=320,contract='400x240像素中心S16F14、Y朝上、单次RNE；真实pROM INIT与便携表均直接解析复核')

def audit_fit(base,native):
    receipt=json.loads((base/'receipt.json').read_text())
    fit=base/f'fits/selected-native-p{receipt["placement"]}-m54';source=base/'source/selected/native.v'
    assert SHA(source)==SHA(fit/'dut.v')==receipt['source_sha256']==receipt['snapshot_sha256']
    assert SHA(fit/'top.v')==receipt['wrapper_sha256']
    strip=lambda s:re.sub(r'\s+','',re.sub(r'//[^\n]*|/\*.*?\*/','',s,flags=re.S))
    sim=Path(native).read_text();sim=re.sub(r'\bmodule\s+gpu_v2_vliw\s*\(','module probe(',sim,count=1)
    assert strip(sim)==strip(source.read_text()),'拟合与仿真必须仅有顶层改名/空格变化'
    ports=top_ports(Path(native).read_text());wrapper=(fit/'top.v').read_text()
    connections={k:(bus,int(start),int(width)) for k,bus,start,width in re.findall(r'\.(\w+)\((src|sink)\[(\d+)\s*\+:\s*(\d+)\]\)',wrapper)}
    expected={n for n in ports if n not in ('clk','ce','reset')};assert set(connections)==expected
    bits={'src':set(),'sink':set()}
    for n,(bus,start,width) in connections.items():
        assert width==ports[n]['width'] and bus==('src' if ports[n]['direction']=='input' else 'sink')
        span=set(range(start,start+width));assert not bits[bus]&span;bits[bus]|=span
    assert bits['src']==set(range(453)) and bits['sink']==set(range(214))
    pnr=fit/'impl/pnr';rpt=(pnr/'leaf.rpt.txt').read_text();tr=(pnr/'leaf.tr').read_text()
    assert SHA(pnr/'leaf.rpt.txt')==receipt['report_sha256'] and SHA(pnr/'leaf.tr')==receipt['timing_sha256']
    for key,pat in [('logic',r'^\s*Logic\s*\|\s*(\d+)/'),('register',r'^\s*Register\s*\|\s*(\d+)/'),('bsram',r'^\s*BSRAM\s*\|\s*(\d+)/'),('ssram',r'--SSRAM\(RAM16\)\s*\|\s*(\d+)'),('dsp',r'^\s*DSP\s*\|\s*(\d+)/')]:
        got=int(re.search(pat,rpt,re.M)[1]);assert got==receipt[key],(key,got,receipt[key])
    setup=int(re.search(r'<Numbers of Setup Violated Endpoints>:(\d+)',tr)[1]);hold=int(re.search(r'<Numbers of Hold Violated Endpoints>:(\d+)',tr)[1])
    assert setup==receipt['setup_violations'] and hold==receipt['hold_violations']
    f=re.search(r'^\s*1\s+clk\s+54\.000\(MHz\)\s+([\d.]+)\(MHz\)',tr,re.M);assert f
    fmax=float(f[1]);assert fmax==receipt['fmax']
    return dict(receipt=str(base/'receipt.json'),receipt_sha256=SHA(base/'receipt.json'),fit_source_sha256=SHA(source),simulation_source_sha256=SHA(native),source_equivalence='仅顶层改名与空格',probe_input_bits=453,probe_output_bits=214,boundary_ff=459,
                logic=receipt['logic'],register=receipt['register'],bsram=receipt['bsram'],ram16=receipt['ssram'],dsp_physical_slots=receipt['dsp'],primitive_counts_from_fit_receipt=receipt['primitives'],fmax=fmax,setup_violations=setup,hold_violations=hold,timing_qualified=not setup and not hold and fmax>=54)

def main():
    ap=argparse.ArgumentParser(__doc__);ap.add_argument('--tag',action='append',required=True);ap.add_argument('--output',required=True);ap.add_argument('--fit');ap.add_argument('--error-contract');ap.add_argument('--history-native',action='append',default=[]);a=ap.parse_args()
    rows=[];source_sets={}
    for tag in a.tag:
        base=ROOT/'target/gpu-vliw-integration-20261018/stage2'/tag
        receipt=json.loads((base/'receipt.json').read_text());assert receipt['status']=='pass'
        manifest=json.loads((base/'source-manifest.json').read_text())
        for rel,h in manifest.items():assert SHA(base/'source'/rel)==h,('frozen source',rel)
        for row in receipt['runs']:
            run=base/row['mode'];assert row['status']=='pass'
            for file,key in [('dut.v','source_sha256'),('front.txt','front_sha256'),('back.txt','back_sha256'),('memory.mem','memory_sha256')]:assert SHA(run/file)==row[key]
            assert SHA(base/'source-manifest.json')==row['source_manifest_sha256']
            assert (run/'memory.mem').read_bytes()==(run/'gold.mem').read_bytes()
            ports=top_ports((run/'dut.v').read_text());widths={d:sum(p['width'] for p in ports.values() if p['direction']==d) for d in ('input','output')};ndc=audit_ndc((run/'dut.v').read_text())
            assert widths['input']>300 and widths['output']>100
            c=row['counts'];assert c['attributes']==12*c['quads'] and c['depth_events']==4*c['quads'] and c['lod']==c['quads']
            assert c['quads']==c['geometry_quads']+c['empty'] and c['live_pixels']==c['ROP_pass']+c['ROP_depth_reject']
            assert c['light']==c['final']==c['live_pixels']
            metrics={k:int(v) for k,v in re.findall(r'(\w+)=(\d+)',row['log'].splitlines()[0])}
            assert metrics['Q']==c['quads'] and metrics['empty']==c['empty']
            fs,bs=spans(run/'front.txt'),spans(run/'back.txt')
            source_sets.setdefault(row['mode'],set()).add(row['source_sha256'])
            failstop='wirecoalition_allow=compute_ce;' in re.sub(r'\s+','',(run/'dut.v').read_text())
            rows.append(dict(tag=tag,mode=row['mode'],receipt_sha256=SHA(run/'receipt.json'),dut_sha256=row['source_sha256'],counts=c,wall_metrics=metrics,front_events=fs,backend_events=bs,top_port_bits=widths,ndc_rom=ndc,error_policy='registered fail-stop with leaf-local rejection' if failstop else 'legacy combinational coalition preflight'))
        if len(receipt['runs'])==2:
            p,n=receipt['runs']
            for key in ('front_sha256','back_sha256','memory_sha256','counts'):assert p[key]==n[key]
    out=Path(a.output);out.mkdir(parents=True,exist_ok=True)
    fits=audit_fit(Path(a.fit),next(ROOT/'target/gpu-vliw-integration-20261018/stage2'/r['tag']/r['mode']/'dut.v' for r in rows if r['mode']=='native')) if a.fit else None
    error_evidence=None
    if a.error_contract and all(r['error_policy'].startswith('registered') for r in rows):
        path=Path(a.error_contract);e=json.loads(path.read_text());assert e['status']=='pass'
        for tag in a.tag:
            manifest=json.loads((ROOT/'target/gpu-vliw-integration-20261018/stage2'/tag/'source-manifest.json').read_text())
            verified=0
            for rel,h in e.get('sources',e.get('source_sha256',{})).items():
                if rel in manifest:assert manifest[rel]==h,('错误资格与完整回放源不同',rel);verified+=1
            assert verified>=4,'必须实核backend/pool四个硬件来源，不能静默遗漏'
        error_evidence=dict(path=str(path),sha256=SHA(path),scope='作者非法协议/外abort/内存排空局部资格；本审计绑定硬件源及收据，不冒称重跑这些定向。')
    historical=[]
    for p in a.history_native:
        run=Path(p);old=json.loads((run/'receipt.json').read_text());assert old['status']=='pass' and old['mode']=='native'
        current=next(r for r in rows if r['mode']=='portable' and r['counts']==old['counts'])
        now=ROOT/'target/gpu-vliw-integration-20261018/stage2'/current['tag']/'portable'
        for file,key in [('dut.v','source_sha256'),('front.txt','front_sha256'),('back.txt','back_sha256'),('memory.mem','memory_sha256')]:assert SHA(run/file)==old[key]
        for file in ('front.txt','back.txt','memory.mem'):assert (run/file).read_bytes()==(now/file).read_bytes()
        historical.append(dict(path=str(run),receipt_sha256=SHA(run/'receipt.json'),old_native_source_sha256=old['source_sha256'],current_portable_source_sha256=current['dut_sha256'],same_events_and_framebuffer=True,scope='不同硬件源的相同合法用例观察等价；不是新版完整箱体原语仿真。'))
    audit=dict(status='pass',auditor_sha256=SHA(__file__),runs=rows,fit=fits,error_evidence=error_evidence,historical_native=historical,source_sets={k:sorted(v) for k,v in source_sets.items()},
        scope='完整正式工厂，按接受事件验证；计数为独立meshlet任务而非一帧。请求数是有限128B事务，ROP通过/拒绝是实际LESS与完整FB金值；事件跨度重叠，不能相加。')
    (out/'audit.json').write_text(json.dumps(audit,ensure_ascii=False,indent=2))
    lines=['# 阶段2独立完整回放审查','',audit['scope'],'',
      '| 输入/刺激 | 模型 | 总wall沿 | 有效CE | quad（空） | 实际像素计算 | ROP通过/拒绝 | 峰值槽 | 128B事务 |',
      '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |']
    for r in rows:
        c,m=r['counts'],r['wall_metrics'];lines.append(f'| {r["tag"]} | {r["mode"]} | {m["cycles"]} | {m["CE"]} | {c["quads"]}（{c["empty"]}） | {c["live_pixels"]} | {c["ROP_pass"]}/{c["ROP_depth_reject"]} | {m["peak"]} | {m["requests"]} |')
    lines+=['','所有V六字、Triangle原19字、H30几何/空quad、I自然lane属性/Z/最终LOD均已逐项比较。Lighting使用主仓独立整数模型及真实N12与像素中心；采样使用最终LOD码，无helper推导。最终纹理与两帧缓冲共400384字节全部一致。','',
      '箱体单实例与兔子局部meshlet均不是完整场景帧；RGB复画是同geometry/attr32的显式模式切换定向测试。全局参数保持不变，I texlog=6匹配实际64×64纹理。纹理UV来自程序生成，并非兔子原贴图。','',
      '压力回放包含有限内存长暂停、输出/CE暂停、全部V后全部T、真实旧pool存活跨批启动、实际LESS深度拒绝。空quad没有Lighting/Sampling/Final输出；源码中两个分支的零mask跳过门也已审阅。','',
      '总wall沿包含开机ROM装载、DMA/V/T/R/I/后半/最终flush。前后半重叠，不能把事件跨度相加或以任务总沿宣称稳态II。本组固定diffuse、specular=0、alpha255、blend关、LESS及深度写，未扩展为所有材料/混合模式证明。未覆盖物理SDRAM仲裁器、CPU软件驱动、显示与板测。']
    if fits:lines+=['',f'原始PnR及完整453输入/214输出探针已复核。54MHz资格：{fits["timing_qualified"]}；数据只引用同源收据 `{fits["receipt"]}`，包含459个探针FF，不能默扣或混作无探针核心。']
    if all(r['error_policy'].startswith('registered') for r in rows):
        lines+=['','合体错误语义：本版保留各leaf本地ready、credit、冲突及非法写入拒绝；顶层在物理沿把诊断union锁入既有sticky stopped，下一沿全链停止，已接受的内存事务继续付费排空。它不承诺原跨全链组合preflight在同一沿撤销所有其它合法操作。外部abort仍立即停止计算。本组是合法输入完整整数资格，非法定向的独立作者收据另行绑定。']
    if error_evidence is None:lines+=['','本次没有绑定故障注入收据；合法输入回放不构成故障资格。需要时显式传入 --error-contract，不能自动借用另一个硬件版本的历史收据。']
    if historical:lines+=['','完整箱体原语回放来自前一冻结硬件版本，已经与本版完整便携回放逐沿事件、数值、最终内存比较完全相同。新版物理池原语另有作者96Q回绕资格，新版整体原语只运行兔子压力用例。不能把组合证据写成新版完整箱体原语测试已经执行。']
    (out/'review.md').write_text('\n'.join(lines)+'\n',encoding='utf8');print(json.dumps(dict(status='pass',runs=len(rows),output=str(out)),ensure_ascii=False))

if __name__=='__main__':main()
