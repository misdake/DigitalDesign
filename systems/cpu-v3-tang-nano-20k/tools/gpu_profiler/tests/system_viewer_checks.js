// Exercise the actual system-source canvas renderer and half-open shared bins.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const nodes=new Map(),noop=()=>{};
function el(id){if(nodes.has(id))return nodes.get(id);const draws=[],listeners={};
 const g=new Proxy({fillRect:(...v)=>draws.push(v)},{get:(o,k)=>k in o?o[k]:noop,set:(o,k,v)=>(o[k]=v,true)});
 const children=[];
 const n={style:{},clientWidth:1000,draws,listeners,children,parentElement:{},append:(...items)=>children.push(...items),replaceChildren:(...items)=>{children.length=0;children.push(...items);},setAttribute:(k,v)=>{n[k]=v;if(k==='id')nodes.set(v,n);},setPointerCapture:noop,getContext:()=>g,addEventListener:(k,f)=>(listeners[k]??=[]).push(f)};nodes.set(id,n);return n;
}
const source=fs.readFileSync(path.join(__dirname,'../system_viewer.js'),'utf8');
const D=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));
let elementId=0;const resized=[];
const context={document:{getElementById:el,createElement:id=>el('created-'+id+'-'+elementId++),createTextNode:text=>({textContent:text}),querySelector:el,querySelectorAll:()=>[el('heading0'),el('heading1')]},window:{devicePixelRatio:1},ResizeObserver:class{constructor(fn){resized.push(fn);}observe(){}},console};
vm.createContext(context);vm.runInContext(source+'\nrenderSystemProfile('+JSON.stringify(D)+');',context,{timeout:15000});
const ui=context.window.systemProfiler,S=D.system;
function sums(){return ui.bins().reduce((a,b)=>{a[0]+=b.clients.reduce((s,v)=>s+v[0]+v[1],0);a[1]+=b.native[0]+b.native[1];return a;},[0,0]);}
const F=S.frame_view,a=F?.start_tick??S.origin_tick,b=F?.end_tick??S.end_tick;
const expectedClient=S.client_payload.filter(v=>a<=v[0]&&v[0]<b).reduce((s,v)=>s+v[2]+v[3],0),expectedNative=S.native_model32.filter(v=>a<=v[0]&&v[0]<b).reduce((s,v)=>s+v[1]+v[2],0);
assert.equal(sums()[0],expectedClient);assert.equal(sums()[1],expectedNative);
assert.ok(el('load').draws.length>0);assert.ok(el('detail').append);assert.ok(el('identity').textContent.includes(F?'Presentation':'Display'));
if(F){
 assert.equal(ui.bins()[0].start,a);assert.equal(ui.bins().at(-1).end,b);
 const selector=el('frame-preview').children.find(n=>n['aria-label']==='Registered scanout frame');assert.equal(selector.children.length,1);
 assert.ok(el('command-summary').textContent.includes(F.owned_submission_ids.length+' frame-owned submits'));
 assert.ok(el('frame-caption').textContent.includes('Presentation '+F.presentation));
 for(const fn of resized)fn();assert.equal(ui.bins()[0].start,a);assert.equal(ui.bins().at(-1).end,b);
 ui.fit(S.origin_tick,S.end_tick);assert.equal(ui.bins()[0].start,a);assert.equal(ui.bins().at(-1).end,b);
 assert.ok(ui.commands().every(c=>F.owned_submission_ids.includes(c.submission)));
}
const event=S.client_payload.find(v=>a<=v[0]&&v[0]<b);
if(event){ui.fit(event[0],event[0]+1);assert.equal(sums()[0],S.client_payload.filter(v=>v[0]===event[0]).reduce((s,v)=>s+v[2]+v[3],0));if(event[0]>a){ui.fit(event[0]-1,event[0]);assert.equal(sums()[0],0);}}
ui.fit();
for(const f of el('load').listeners.mousemove)f({offsetX:500});
assert.ok(el('cursor').textContent.includes('Outstanding means'));
for(const f of el('load').listeners.wheel)f({offsetX:500,deltaY:-1,preventDefault:noop});
assert.ok(ui.bins()[0].start>a || ui.bins().at(-1).end<b);
if(D.commands.length){
 for(const fn of el('filter-all').listeners.click)fn();
 const candidates=F?D.commands.filter(c=>F.owned_submission_ids.includes(c.submission)):D.commands;
 assert.equal(ui.commands().length,candidates.length);
 if(candidates.length){ui.select(candidates[0].id);assert.ok(el('cb-inspector').textContent.includes(candidates[0].word));}
 for(const fn of el('filter-none').listeners.click)fn();assert.equal(ui.commands().length,0);
 for(const fn of el('filter-default').listeners.click)fn();
}
console.log('System viewer passed: real canvas, separate native/client byte bins, half-open boundaries, shared zoom/hover and outstanding/wait accounting.');
