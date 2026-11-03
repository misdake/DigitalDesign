// Exercise the actual system-source canvas renderer and half-open shared bins.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const nodes=new Map(),noop=()=>{};
function el(id){if(nodes.has(id))return nodes.get(id);const draws=[],listeners={};
 const g=new Proxy({fillRect:(...v)=>draws.push(v)},{get:(o,k)=>k in o?o[k]:noop,set:(o,k,v)=>(o[k]=v,true)});
 const n={style:{},clientWidth:1000,draws,listeners,parentElement:{},append:noop,setAttribute:noop,setPointerCapture:noop,getContext:()=>g,addEventListener:(k,f)=>(listeners[k]??=[]).push(f)};nodes.set(id,n);return n;
}
const source=fs.readFileSync(path.join(__dirname,'../system_viewer.js'),'utf8');
const D=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));
const context={document:{getElementById:el,createElement:id=>el('created-'+id),querySelector:el,querySelectorAll:()=>[el('heading0'),el('heading1')]},window:{devicePixelRatio:1},ResizeObserver:class{observe(){}},console};
vm.createContext(context);vm.runInContext(source+'\nrenderSystemProfile('+JSON.stringify(D)+');',context,{timeout:15000});
const ui=context.window.systemProfiler,S=D.system;
function sums(){return ui.bins().reduce((a,b)=>{a[0]+=b.clients.reduce((s,v)=>s+v[0]+v[1],0);a[1]+=b.native[0]+b.native[1];return a;},[0,0]);}
const expectedClient=S.client_payload.reduce((s,v)=>s+v[2]+v[3],0),expectedNative=S.native_model32.reduce((s,v)=>s+v[1]+v[2],0);
assert.equal(sums()[0],expectedClient);assert.equal(sums()[1],expectedNative);
assert.ok(el('load').draws.length>0);assert.ok(el('detail').append);assert.ok(el('identity').textContent.includes('Display'));
const event=S.client_payload[0];
if(event){ui.fit(event[0],event[0]+1);assert.equal(sums()[0],S.client_payload.filter(v=>v[0]===event[0]).reduce((s,v)=>s+v[2]+v[3],0));ui.fit(event[0]-1,event[0]);assert.equal(sums()[0],0);}
ui.fit();
for(const f of el('load').listeners.mousemove)f({offsetX:500});
assert.ok(el('cursor').textContent.includes('Outstanding means'));
for(const f of el('load').listeners.wheel)f({offsetX:500,deltaY:-1,preventDefault:noop});
assert.ok(ui.bins()[0].start>S.origin_tick || ui.bins().at(-1).end<S.end_tick);
console.log('System viewer passed: real canvas, separate native/client byte bins, half-open boundaries, shared zoom/hover and outstanding/wait accounting.');
