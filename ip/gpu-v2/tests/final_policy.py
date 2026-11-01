"""Actual selected Final/shared-DSP bounded policy and numeric qualification."""
from pathlib import Path
import argparse,hashlib,json,os,random,re,subprocess

def module(source,name):
    return re.search(r'\bmodule\s+'+re.escape(name)+r'(?=[\s(#]).*?\bendmodule\b',source,re.S)[0]

def vectors():
    rng=random.Random(5051101);pixels=[];gold=[]
    for i in range(96):
        t=[rng.randrange(256) for _ in range(3)];x=[rng.randrange(256) for _ in range(3)];s=[rng.randrange(256) for _ in range(3)]
        if i<12:t=[0,1,2,63,64,127,128,129,253,254,255,255][i:i+1]*3;x=[255,254,129][i%3:i%3+1]*3
        g=[0,1,255,256,257,510,511][i%7];h=[0,1,127,128,255,256][i%6]
        pack=lambda a:sum(v<<(8*j)for j,v in enumerate(a))
        pixels.append(pack(t)|(pack(x)<<24)|(pack(s)<<48)|(g<<72)|(h<<81))
        # Independent integer recipe: nearest /255, separate floor /256 terms,
        # then saturation. This is the selected fixed recipe, not the ideal RNE sum.
        gold.append(pack([min(255,((a*b+127)//255*g)//256+(c*h)//256)for a,b,c in zip(t,x,s)]))
    return pixels,gold

def call(command,path):
    q=subprocess.run(list(map(str,command)),capture_output=True,text=True,timeout=60);path.write_text(q.stdout+q.stderr);return q

def compile_case(source,out,native,synthesis,checked):
    out.mkdir(parents=True);(out/'dut.v').write_text(source);bench=Path(__file__).with_name('fixtures')/'final_policy_tb.v'
    library=Path(os.environ['GOWIN_HOME'])/'IDE/simlib/gw2a/prim_sim.v'
    args=[os.environ['IVERILOG_EXE'],'-g2012','-s','tb','-Ptb.CHECKED='+str(int(checked)),'-Ptb.MONITORS='+str(int(not synthesis)),*(['-DGPU_V2_GOWIN_DSP']if native else[]),*(['-DSYNTHESIS']if synthesis else[]),'-o',out/'sim.vvp',out/'dut.v',bench,library]
    q=call(args,out/'compile.log');assert q.returncode==0,q.stdout+q.stderr
    return out/'sim.vvp'

def run(exports,out,negatives=False):
    exports=exports.resolve();out=out.resolve();out.mkdir(parents=True,exist_ok=False);pixels,gold=vectors();(out/'pixels.mem').write_text(''.join(f'{v:023x}\n'for v in pixels));(out/'gold.mem').write_text(''.join(f'{v:06x}\n'for v in gold));rows=[];sources={}
    for profile in ['command','generic']:
        for native in[False,True]:
            label=profile+('-native'if native else'-portable');whole=(exports/(label+'.v')).read_text();source='\n'.join(module(whole,n)for n in['gpu_v2_final_ordered_two_cycle','gpu_v2_final_two_cycle_mul9','gpu_v2_final_rop_bank']);sources[label]=source
            for synthesis in[False,True]:
                d=out/f'{label}-s{int(synthesis)}';exe=compile_case(source,d,native,synthesis,profile=='generic')
                for scenario in['legal','abort','h','credit']:
                    q=call([os.environ['VVP_EXE'],exe,'+INPUT='+str(out/'pixels.mem'),'+GOLD='+str(out/'gold.mem'),'+CASE='+scenario],d/(scenario+'.log'))
                    fatal=scenario in['h','credit']and not synthesis and(profile=='command'or scenario=='credit')
                    if fatal:
                        assert q.returncode!=0 and 'FATAL' in q.stdout,(label,scenario,q.stdout)
                        assert any(t in q.stdout for t in['GPU contract: preflight_bad','Final old reservation','Final reservation']),q.stdout
                        timing=re.search(r'Time: (\d+)',q.stdout);assert timing
                        result={'expected_rejection':True,'failure_time':timing[1]}
                    else:
                        assert q.returncode==0 and'PASS FINAL'in q.stdout,(label,scenario,q.stdout+q.stderr)
                        trace='\n'.join(l for l in q.stdout.splitlines()if l.startswith(('E ','RGB ','OBS ')))
                        result={'trace_sha256':hashlib.sha256(trace.encode()).hexdigest(),'pass':next(l for l in q.stdout.splitlines()if l.startswith('PASS FINAL'))}
                    rows.append({'profile':profile,'native':native,'synthesis':synthesis,'case':scenario,**result})
    for scenario in['legal','abort']:
        assert len({r['trace_sha256']for r in rows if r['case']==scenario})==1,scenario
    # The raw template still defaults to the original checked contract.
    raw_template = (Path(__file__).resolve().parents[1] / 'src/system/pixel/vliw_backend_final.v').read_text()
    plain = raw_template + '\n' + '\n'.join(module(sources['generic-portable'], n)
                for n in ['gpu_v2_final_two_cycle_mul9', 'gpu_v2_final_rop_bank'])
    exe = compile_case(plain, out / 'raw-template-checked', False, True, True)
    raw_cases = []
    for scenario in ['legal', 'h']:
        q = call([os.environ['VVP_EXE'], exe, '+INPUT=' + str(out / 'pixels.mem'),
                  '+GOLD=' + str(out / 'gold.mem'), '+CASE=' + scenario],
                 out / 'raw-template-checked' / (scenario + '.log'))
        assert q.returncode == 0 and 'PASS FINAL' in q.stdout, q.stdout + q.stderr
        raw_cases.append(scenario)
    bad=[]
    if negatives:
        original=sources['command-native'];mutants=[('value','assign color[c*8+:8]=saturated?8\'d255:rounded;',"assign color[c*8+:8]=(saturated?8'd255:rounded)^8'd1;",'legal','RGB order/value'),('ack-credit','&&old_slot_free;','&&(old_slot_free||out_ready);','legal','borrowed ACK credit'),('mul-clock','wire mul_ce=advance&&','wire mul_ce=ce&&','legal','held arithmetic'),('abort-ce','else if(abort)stopped<=1;','else if(abort&&ce)stopped<=1;','abort','abort sticky')]
        for name,old,new,scenario,reason in mutants:
            assert original.count(old)==1,(name,original.count(old));exe=compile_case(original.replace(old,new,1),out/name,True,True,False);q=call([os.environ['VVP_EXE'],exe,'+INPUT='+str(out/'pixels.mem'),'+GOLD='+str(out/'gold.mem'),'+CASE='+scenario],out/name/'run.log');assert q.returncode!=0 and reason in q.stdout,(name,q.stdout);bad.append({'name':name,'caught':True,'reason':reason})
    receipt={'passed':True,'exports':str(exports),'cases':rows,'negative_controls':bad,'raw_template_checked_cases':raw_cases,'cycle_limit':3000,'inputs':96,'sources_sha256':{k:hashlib.sha256(v.encode()).hexdigest()for k,v in sources.items()},'fixture_sha256':hashlib.sha256(Path(__file__).with_name('fixtures').joinpath('final_policy_tb.v').read_bytes()).hexdigest(),'scope':'Actual Final/mul9/shared Final-ROP bank; independent integer values and external credits/order; no pool/TDP/frame/board. Invalid RGB is unobserved when out_valid=0.'};(out/'receipt.json').write_text(json.dumps(receipt,indent=2));print(json.dumps({'passed':True,'cases':len(rows),'negatives':len(bad)}))

def compare_profiles(baseline, exports, output, baseline_results, candidate_results):
    """Full-stream checks; only two exact, proven-dead legacy statements differ."""
    output.mkdir()
    lex = re.compile(r'"(?:\\.|[^"\\])*"|//[^\n]*|/\*.*?\*/|[A-Za-z_$][A-Za-z0-9_$]*|\d+|[^\s]', re.S)

    def tokens(text):
        return [x for x in lex.findall(text) if not x.startswith(("//", "/*"))]

    def pp(source, path, synthesis):
        path.with_suffix('.input.v').write_text(source)
        q = call([os.environ['IVERILOG_EXE'], '-g2012', '-E',
                  *(['-DSYNTHESIS'] if synthesis else []), '-o', path,
                  path.with_suffix('.input.v')], path.with_suffix('.log'))
        assert q.returncode == 0, q.stdout + q.stderr
        return path.read_text()

    def replace_exact_once(actual, old, replacement):
        positions = [i for i in range(len(actual) - len(old) + 1)
                     if actual[i:i + len(old)] == old]
        assert len(positions) == 1, 'Named legacy statement not uniquely present'
        i = positions[0]
        return actual[:i] + replacement + actual[i + len(old):]

    rows = []
    files = sorted(baseline.glob('*.v'))
    assert len(files) == 8, 'Freeze the eight actual factory profiles'
    for source in files:
        for synthesis in [False, True]:
            tag = source.stem + '-s' + str(int(synthesis))
            old = pp(source.read_text(), output / (tag + '-base.pp'), synthesis)
            new = pp((exports / source.name).read_text(), output / (tag + '-new.pp'), synthesis)
            complete = tokens(old) == tokens(new)
            if not complete:
                assert source.name.startswith('command-'), 'Checked/public profile changed'
                pattern = r'\bmodule gpu_v2_final_ordered_two_cycle(?=[\s(#]).*?\bendmodule\b'
                a, b = re.search(pattern, old, re.S), re.search(pattern, new, re.S)
                assert tokens(old[:a.start()] + old[a.end():]) == tokens(new[:b.start()] + new[b.end():])
                assert "assign preflight_bad=1'b0;" in a[0]
                actual = replace_exact_once(tokens(a[0]),
                    tokens('if(reset)begin begin end arithmetic_live<=0;'),
                    tokens('if(reset)begin arithmetic_live<=0;'))
                # The old branch can run only with abort=1 (preflight is constant0),
                # while its monitor requires !abort. No arbitrary statements removed.
                dead = 'if(abort||preflight_bad)begin '
                if not synthesis:
                    dead += 'if(!reset&&!abort&&((1)!=0))$fatal(1,"GPU contract: stopped");'
                dead += ' end'
                actual = replace_exact_once(actual, tokens(dead), [])
                assert actual == tokens(b[0]), 'Final differs beyond the two named dead statements'
            rows.append(dict(profile=source.name, synthesis=synthesis,
                             complete_ordered_stream_equal=complete,
                             named_dead_statement_only_exception=not complete))

    before = json.loads(baseline_results.read_text())
    assert before['cases'] == candidate_results['cases'], 'Actual values/cycles/diagnostic edges changed'
    assert before['fixture_sha256'] == candidate_results['fixture_sha256'], 'Fixture bytes changed'
    for name in ['pixels.mem', 'gold.mem']:
        assert (baseline_results.parent / name).read_bytes() == (output.parent / name).read_bytes()
    for profile in ['command', 'generic']:
        for backend in ['native', 'portable']:
            name = profile + '-' + backend
            raw = (baseline / (name + '.v')).read_text()
            dut = '\n'.join(module(raw, n) for n in ['gpu_v2_final_ordered_two_cycle',
                              'gpu_v2_final_two_cycle_mul9', 'gpu_v2_final_rop_bank'])
            assert hashlib.sha256(dut.encode()).hexdigest() == before['sources_sha256'][name]
    for backend in ['native', 'portable']:
        command = (exports / ('command-' + backend + '.v')).read_text()
        generic = (exports / ('generic-' + backend + '.v')).read_text()
        for synthesis in [False, True]:
            tag = backend + '-s' + str(int(synthesis))
            probe = '\n`ifdef GPU_V2_COMMAND_FINAL\nFINAL_PROFILE_LEAK\n`endif\n'
            boundary = 'module final_scope_boundary;endmodule'
            joined = command + probe + '\n`undef GPU_V2_COMMAND_PORTS\n`undef GPU_V2_GOWIN_DSP\n' + boundary + '\n' + generic + probe
            actual = pp(joined, output / ('scope-' + tag + '.pp'), synthesis)
            assert 'FINAL_PROFILE_LEAK' not in actual
            expected = pp(generic, output / ('generic-' + tag + '.pp'), synthesis)
            assert tokens(actual.split(boundary, 1)[1]) == tokens(expected)
            forced = pp('`define GPU_V2_COMMAND_FINAL\n' + generic + probe,
                        output / ('forced-' + tag + '.pp'), synthesis)
            assert 'FINAL_PROFILE_LEAK' not in forced and tokens(forced) == tokens(expected)
    result = dict(passed=True, streams=rows, behavior_rows_exact=len(before['cases']),
                  fixture_stimulus_gold_exact=True, private_scope_and_forced_generic_checked=True)
    (output / 'receipt.json').write_text(json.dumps(result, indent=2))
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(__doc__)
    p.add_argument('--exports', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--negatives', action='store_true')
    p.add_argument('--baseline', type=Path)
    p.add_argument('--baseline-results', type=Path)
    a = p.parse_args()
    if bool(a.baseline) != bool(a.baseline_results):
        p.error('--baseline and --baseline-results must be supplied together')
    run(a.exports, a.output, a.negatives)
    if a.baseline:
        current = json.loads((a.output / 'receipt.json').read_text())
        compare_profiles(a.baseline.resolve(), a.exports.resolve(),
                         a.output.resolve() / 'equivalence', a.baseline_results.resolve(), current)
        print('PASS exact behavior, public streams, named-noop command proof and private scope')
