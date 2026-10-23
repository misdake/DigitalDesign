// Explicit, reproducible import from the user's read-only Bloxorz reference.
import { pathToFileURL, fileURLToPath } from 'node:url';
import { resolve, join } from 'node:path';
import { readFile, writeFile, mkdir, copyFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';

const source = process.argv[2];
if (!source) throw Error('Usage: node tools/import-reference.mjs <BloxorzClone directory>');
const root = resolve(source);
const out = fileURLToPath(new URL('../assets/', import.meta.url));
const { parseAllLevels, parseLevel } = await import(pathToFileURL(join(root, 'src/core/levels.js')));
const { Game } = await import(pathToFileURL(join(root, 'src/core/game.js')));
const { encodeSnapshot } = await import(pathToFileURL(join(root, 'src/core/state.js')));
const levels = parseAllLevels();
// Exercise order-sensitive stacked operations, fragile support and split merging.
levels.push(parseLevel({ id:'fixture-switch', rows:['########','##SHFBO#','########'],
    start:{x:0,z:1,orient:'standing'},channels:['00000000','00111000'],
    switchDefs:[{x:2,z:1,channels:'01',action:'n'}, {x:2,z:1,channels:'1',action:'t',add:true}],
    solution:[] }));
levels.push(parseLevel({ id:'fixture-split', rows:['########','###P####','########'],
    start:{x:0,z:1,orient:'standing'},splitters:[{x:3,z:1,t1:[1,1],t2:[5,1]}],solution:[0,0,6,1,1] }));

let lines=['# Bloxorz packed levels and independent reference transitions, schema 1.'];
let rng = 0x159a26;
let transitions=0;
for (const level of levels) {
    if(level.width>32||level.height>32||level.width*level.height>256) throw Error('Level bound');
    const words=[level.width,level.height,level.start.x,level.start.z,level.start.orient,level.initialMask,level.width*level.height,1];
    for(let z=0;z<level.height;z++) for(let x=0;x<level.width;x++) {
        const ops=level.switchOpsAt(x,z);
        const apply=(mask)=>ops.reduce((m,o)=>o.action===1?m&~o.bits:o.action===2?m^o.bits:m|o.bits,mask)&4095;
        const zero=apply(0), keep=zero^apply(4095), split=level.splitAt(x,z);
        if(split && [...split.t1,...split.t2].some(v=>v<0||v>31))throw Error('Splitter coordinate bound');
        words.push(level.tileAt(x,z),1<<level.channelAt(x,z),keep,zero,...(split?[...split.t1,...split.t2]:[65535,0,0,0]));
    }
    lines.push(`level ${level.id} ${Number(level.unverified)} ${words.join(' ')}`);
    const game = new Game(level);
    let commands=[4,...level.solution,5,5,4];
    for(let i=0;i<160;i++) {
        rng=(Math.imul(rng,1664525)+1013904223)>>>0;
        commands.push((rng>>>16)%7);
    }
    if(commands.length>512) throw Error(`Trace too long: ${level.id}`);
    for(const command of commands) {
        const result=game.dispatch(command);
        const packed=Array.from(encodeSnapshot(game.state)).flatMap(v=>[v&65535,v>>>16]);
        lines.push(`step ${command} ${Number(result.accepted)} ${packed.join(' ')}`);
        transitions++;
    }
}
await mkdir(out,{recursive:true});
await writeFile(join(out,'reference.txt'),lines.join('\n')+'\n');
await copyFile(join(root,'textures/atlas-low.png'),join(out,'atlas-low.png'));
const files=['src/core/constants.js','src/core/state.js','src/core/game.js','src/core/levels.js','src/render/camera.js','src/render/ground-shadow.js','textures/atlas-low.png'];
const hashes={};for(const file of files)hashes[file]=createHash('sha256').update(await readFile(join(root,file))).digest('hex');
await writeFile(join(out,'provenance.json'),JSON.stringify({schema:1,source:'User-provided BloxorzClone',files:hashes,levels:33,fixtures:2,transitions,unverified:['b23']},null,2)+'\n');
console.log(JSON.stringify({levels:33,fixtures:2,transitions}));
