"""Generate observation-only copies of the selected functional GPU sources.

No production GPU file is modified. The executable checks every resulting physical
halfword against the public Executor for each frozen real CPU submission.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path
import subprocess


def replace_once(text, old, new):
    assert text.count(old) == 1, (old, text.count(old))
    return text.replace(old, new)


def generate(root, output, deps):
    source = root / 'ip/gpu-v2/src/system'
    names = ['functional.rs', 'functional/geometry.rs', 'functional/shading.rs', 'functional/lighting.rs']
    original = {name: (source/name).read_text(encoding='utf-8') for name in names}
    changed = dict(original)
    changed['functional.rs'] = replace_once(changed['functional.rs'],
        'let result = state.command(memory, word, self.maximum_work);',
        'crate::trace::command(pc, word);\n            let result = state.command(memory, word, self.maximum_work);')
    g = changed['functional/geometry.rs']
    g = replace_once(g, 'for indices in &mesh.triangles {', 'for (triangle_id, indices) in mesh.triangles.iter().enumerate() {')
    g = replace_once(g, '        let mut prepared = independent', r"""
        let mut observations = Vec::new();
        for [x, y] in crate::trace::points() {
            if triangle.span(y).is_some_and(|(lo, hi)| (lo..=hi).contains(&x)) {
                let fragment = triangle.fragment(x, y, state.global.far)?;
                let quad = Quad::new(x & !1, y & !1, [triangle.span(y & !1), triangle.span((y & !1) + 1)]);
                let fragments = quad.fragments(&triangle, state.global.far)?;
                let first = fragments.iter().flatten().next().unwrap();
                let lod = if mesh.rgb {0} else {triangle.lod(first, state.render)};
                let detail = serde_json::json!({
                    "triangle": triangle_id, "indices": indices,
                    "position_q16": triangle.vertices.map(|v|v.position),
                    "attributes": triangle.vertices.map(|v|v.attribute),
                    "edges":triangle.edges, "edge_exponents":triangle.exponents,
                    "depth_mantissa":triangle.depth_mantissa,"depth_shift":triangle.depth_shift,
                    "uniform": uniform, "mesh_origin_q16":mesh.origin,"mesh_shift":mesh.shift,
                    "cull":mesh.cull,"mesh_rgb":mesh.rgb,"valid":fragment.valid
                });
                observations.push(shading::trace_before(memory,[x,y],&fragment,&state,lod,detail)?);
            }
        }
        let mut prepared = independent""")
    g = replace_once(g, '        report.charge((120 - active_rows.end) * 2, maximum)?;',
        '        for observation in observations { shading::trace_after(memory, observation)?; }\n        report.charge((120 - active_rows.end) * 2, maximum)?;')
    changed['functional/geometry.rs'] = g
    s = changed['functional/shading.rs']
    s = replace_once(s, '        Ok(expand(\n            memory.load(self.base + self.prefix(level) + offset)?,\n        ))',
        r"""        let address = self.base + self.prefix(level) + offset;
        let value = memory.load(address)?;
        crate::trace::tap(serde_json::json!({"level_log2":level,"xy":[x,y],"physical_byte":address*2,"rgb565":value,"expanded":expand(value)}));
        Ok(expand(value))""")
    s = replace_once(s, '                let color =\n                    self.texel(memory, level, x + (tap & 1) as i128, y + (tap >> 1) as i128)?;',
        r"""                crate::trace::tap(serde_json::json!({"tap":tap,"weight_unorm9":weight,"sample_xy_q8":coordinates,"parent_weight":parent}));
                let color =
                    self.texel(memory, level, x + (tap & 1) as i128, y + (tap >> 1) as i128)?;""")
    helpers=Path(__file__).with_name('pixel_trace_shading.rs').read_text(encoding='utf-8')
    s += helpers
    changed['functional/shading.rs'] = s
    output.mkdir(parents=True,exist_ok=True)
    for name, text in changed.items():
        dest=output/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_text(text,encoding='utf-8')
    numeric=(source.parent/'frontend/vliw_numeric.rs').read_text(encoding='utf-8').split('pub(super) fn verilog()')[0].replace('//!','//').replace('use std::fmt::Write;','')
    (output/'numeric.rs').write_text(numeric,encoding='utf-8')
    raw={}
    for file,constant_names in [('lighting.rs',['RSQRT_Q13_RAW','RSQRT_Q13_BIASES']),('lighting_h9.rs',['H9_RAW','H9_SHIFTS_RAW'])]:
        candidates=list(deps.parent.joinpath('build').glob('gpu-v2-*/out/'+file));assert candidates,file
        for name in constant_names:
            values=[]
            for candidate in candidates:
                match=re.search(r'pub const '+name+r':[^=]+?=\s*\[.*?\];',candidate.read_text(encoding='utf-8'),re.S);assert match,name
                values.append(match.group())
            assert len(set(values))==1,(name,'ambiguous cached generated constants')
            raw[name]=values[0]
    (output/'lighting_constants.rs').write_text(raw['RSQRT_Q13_RAW']+'\n'+raw['RSQRT_Q13_BIASES']+'\npub(crate) mod h9 {\n'+raw['H9_RAW']+'\n'+raw['H9_SHIFTS_RAW']+'\n}\n',encoding='utf-8')
    (output/'main.rs').write_text(Path(__file__).with_name('pixel_trace_main.rs').read_text(encoding='utf-8'),encoding='utf-8')
    support=['numeric.rs','lighting_constants.rs','main.rs']
    (output/'support-identity.json').write_text(json.dumps({name:hashlib.sha256((output/name).read_bytes()).hexdigest() for name in support},indent=2)+'\n',encoding='utf-8')
    (output/'source-identity.json').write_text(json.dumps({name:{'source_sha256':hashlib.sha256((source/name).read_bytes()).hexdigest(),'instrumented_sha256':hashlib.sha256((output/name).read_bytes()).hexdigest()} for name in names},indent=2)+'\n',encoding='utf-8')


if __name__ == '__main__':
    p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--deps',type=Path,required=True);a=p.parse_args()
    generate(a.root.resolve(),a.output.resolve(),a.deps.resolve())
    command=['rustc','--edition=2021','-O',str(a.output/'main.rs'),'-L','dependency='+str(a.deps),'-o',str(a.output/'pixel-trace.exe')]
    for name in ['gpu_v2','rayon','serde_json']:
        files=sorted(a.deps.glob('lib'+name+'-*.rlib'));assert files,name
        if name=='gpu_v2':assert len(files)==1,'Use a dependency directory with one selected GPU feature build'
        command += ['--extern',name+'='+str(files[0])]
    (a.output/'compiler-inputs.json').write_text(json.dumps({'command':command,'rlib_sha256':{str(path):hashlib.sha256(path.read_bytes()).hexdigest() for name in ['gpu_v2','rayon','serde_json'] for path in [sorted(a.deps.glob('lib'+name+'-*.rlib'))[0]]}},indent=2)+'\n',encoding='utf-8')
    subprocess.run(command,check=True)
