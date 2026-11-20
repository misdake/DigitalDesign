"""Build a finite atlas-LOD observer; physical output must equal public Executor."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
from pixel_trace import generate, replace_once

def build(root,output,deps):
    generate(root,output,deps)
    geometry=output/'functional/geometry.rs'
    text=geometry.read_text()
    text=replace_once(text,'                for (lane, fragment) in fragments.into_iter().enumerate() {','''                let mut footprint_state=state.render;
                footprint_state.texture.full_mip=true;
                crate::atlas_trace::set_footprint(triangle.lod(first,footprint_state));
                crate::atlas_trace::set_expected_uv([(triangle.vertices[0].attribute&65535) as i128,((triangle.vertices[0].attribute>>16)&65535) as i128]);
                for (lane, fragment) in fragments.into_iter().enumerate() {''')
    geometry.write_text(text)
    shading=output/'functional/shading.rs'
    text=shading.read_text()
    text=replace_once(text,'    let color = shade(memory, xy, &fragment, state, lod, report, maximum)?;', '''
    let old_depth=memory.load(state.target[1]+super::column_offset(xy[0],xy[1]))?;
    let passed=match state.render.control&7 {0=>false,1=>fragment.depth<old_depth,2=>fragment.depth==old_depth,3=>fragment.depth<=old_depth,4=>fragment.depth>old_depth,5=>fragment.depth!=old_depth,6=>fragment.depth>=old_depth,_=>true};
    if !state.rgb {crate::atlas_trace::begin(state.render.texture.base*2,state.render.texture.size_log2,state.render.texture.full_mip,state.render.texture.filter,fragment.uv,lod,passed);}
    let color = shade(memory, xy, &fragment, state, lod, report, maximum)?;
    crate::atlas_trace::end();''')
    text=replace_once(text,'        let address = self.base + self.prefix(level) + offset;', '        crate::atlas_trace::tap(level,x,y);\n        let address = self.base + self.prefix(level) + offset;')
    text=replace_once(text,'                crate::trace::tap(serde_json::json!({"tap":tap,', '                crate::atlas_trace::weight(weight);\n                crate::trace::tap(serde_json::json!({"tap":tap,')
    shading.write_text(text)
    helper=Path(__file__).with_suffix('.rs')
    (output/'atlas_trace.rs').write_bytes(helper.read_bytes())
    main=output/'main.rs'
    text=main.read_text().replace('mod functional;','mod functional;\nmod atlas_trace;')
    text=replace_once(text,'        assert_eq!(actual, reference, "observation changed submission {i}");','        assert_eq!(actual, reference, "observation changed submission {i}");\n        atlas_trace::finish(i,output);')
    main.write_text(text)
    command=['rustc','--edition=2021','-O',str(main),'-L','dependency='+str(deps),'-o',str(output/'atlas-trace.exe')]
    for name in ['gpu_v2','rayon','serde_json']:
        files=sorted(deps.glob('lib'+name+'-*.rlib'));assert files,name
        if name=='gpu_v2':assert len(files)==1,(name,files)
        command+=['--extern',name+'='+str(files[0])]
    identities={p.relative_to(output).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in output.rglob('*.rs')}
    (output/'atlas-observer-identity.json').write_text(json.dumps(dict(generated=identities,command=command,source_helper_sha256=hashlib.sha256(helper.read_bytes()).hexdigest(),parallelism='Run with RAYON_NUM_THREADS=1; serial production fragment path'),indent=2)+'\n')
    subprocess.run(command,check=True)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--root',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--deps',type=Path,required=True);a=parser.parse_args()
    build(a.root.resolve(),a.output.resolve(),a.deps.resolve())
