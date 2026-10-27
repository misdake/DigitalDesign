// Bounded application-logic checks with a minimal DOM/canvas adapter.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),path=require('node:path');
const elements=new Map(),noop=()=>{};
function element(id){if(elements.has(id))return elements.get(id);const listeners={},capture=new Set();
 const ctx=new Proxy({measureText:s=>({width:s.length*6})},{get:(o,k)=>k in o?o[k]:noop,set:(o,k,v)=>(o[k]=v,true)});
 const el={id,listeners,style:{},clientWidth:800,offsetHeight:70,scrollTop:0,innerHTML:'',textContent:'',value:'',
 parentElement:{clientWidth:800},getContext:()=>ctx,getBoundingClientRect:()=>({left:0,top:0,width:800,height:2000}),
 querySelector:()=>({style:{}}),addEventListener:(k,fn)=>(listeners[k]??=[]).push(fn),
 setPointerCapture:p=>capture.add(p),hasPointerCapture:p=>capture.has(p),releasePointerCapture:p=>capture.delete(p)};
 elements.set(id,el);return el;}
const keys=['dma','vt_owned','t_queue','quad_total','light_tokens','sample_quads','final_wait','final_tokens','rop_contexts','r_quad','r_busy','i_busy'];
const data={start:0,end:1000000,submit:100,gpu_end:999990,clock_hz:54000000,sample_start:0,sample_end:1000000,
 partial:false,cpu:[],meshlets:[],jobs:[],dma_transactions:[],commands:[0,4,1,3].map((op,i)=>({pc:i*4,opcode:op,name:['NOP','WAIT','DMA_LOCAL','DRAW'][i],start:300000,execute:300010,end:400000,word:'00000000',jobs:[],waits:[],dependencies:[]})),
 metric_defs:keys.map((k,i)=>({key:k,label:k,kind:i<2?'binary':i===9?'rate':'queue',capacity:i<2?1:24,peak:i<2?1:10,integral:400000})),tracks:{}};
for(const d of data.metric_defs)data.tracks[d.key]=d.kind==='rate'?[[400000,1],[400001,1]]:[[0,0],[300000,d.kind==='binary'?1:10],[700000,0]];
for(const [i,name]of ['Rabbit 02','Rabbit 03'].entries())data.meshlets.push({id:i+1,name,start:i?600010:300010,vt_end:i?650000:400000,triangles:0,quads:0,vt:[],r:[],i:[],p:[],stats:{r:{active:0,first:null},i:{active:0,first:null},p:{active:0,first:null},vt:{}}});
data.commands[3].meshlet=1;
data.commands.push({...data.commands[3],pc:16,meshlet:2,start:600000,execute:600010,end:650000});
data.jobs.push({id:1,start:250000,end:260000,transactions:[],source:0});
Object.assign(data.meshlets[0],{dma:1,last_quad:{id:1,end:450000,pool_retire:449999},p:[[390000,450000]]});
data.title='Test';data.submissions=[{id:1,base:4096,start:100,end:999990}];for(const c of data.commands){c.id=c.pc;c.submission=1;}
const context={document:{getElementById:element,documentElement:{},querySelector:()=>element('main')},
 getComputedStyle:()=>({getPropertyValue:k=>k==='--left'?'264px':'#ffffff'}),window:{devicePixelRatio:1},
 ResizeObserver:class{observe(){}},setTimeout:noop,clearTimeout:noop,innerWidth:1200,innerHeight:900,console};
vm.createContext(context);
const html=fs.readFileSync(path.join(__dirname,'../viewer.html'),'utf8');
const script=html.split('<script>')[1].split('</script>')[0].replace('/*PROFILE_DATA*/null',JSON.stringify(data));
vm.runInContext(script,context,{timeout:10000});
function emit(id,type,args={}){for(const fn of element(id).listeners[type]??[])fn({type,button:0,pointerId:7,clientX:500,clientY:10,preventDefault:noop,...args});}
function view(){return context.window.gpuProfiler.getView();}
function evalValue(s){return vm.runInContext(s,context,{timeout:5000});}
assert.ok(!element('instruction').innerHTML.includes('>0004 WAIT'));
assert.ok(!element('instruction').innerHTML.includes('DMA_LOCAL'));
assert.ok(element('instruction').innerHTML.includes('DRAW'));
emit('filter-options','change',{target:{value:'4',checked:true}});
assert.ok(element('instruction').innerHTML.includes('WAIT'));
assert.ok(!html.includes('id="command-query"'));assert.ok(!html.includes('id="filter-min"'));assert.ok(!html.includes('id="filter-window"'));
assert.equal(element('filter-type-count-3').textContent,2);
emit('filter-all','click');assert.equal(element('filter-count').textContent,'(5/5)');
emit('filter-none','click');assert.equal(element('filter-count').textContent,'(0/5)');assert.equal(element('command-empty').hidden,false);assert.equal(view().selected,null);
emit('filter-default','click');assert.equal(element('filter-count').textContent,'(3/5)');
assert.equal(element('command-empty').hidden,true);
assert.equal(view().rangeSelected,false);
const overviewX=t=>264+t/1000000*(800-264-12);
emit('overview','pointerdown',{clientX:overviewX(200000)});emit('overview','pointermove',{clientX:overviewX(500000)});emit('overview','pointerup');
assert.equal(view().lo,200000);assert.equal(view().hi,500000);assert.equal(view().rangeSelected,true);
context.window.gpuProfiler.fit();assert.equal(view().rangeSelected,false);
evalValue('fitRange(200000,500000)');emit('overview','dblclick',{clientX:overviewX(350000)});assert.equal(view().rangeSelected,true);
emit('overview','dblclick',{clientX:overviewX(700000)});assert.equal(view().rangeSelected,false);
evalValue('fitRange(200000,500000)');emit('overview','pointerdown',{button:2});emit('overview','pointerup',{button:2});assert.equal(view().rangeSelected,false);
evalValue('fitRange(200000,500000)');emit('overview','pointerdown',{button:2});emit('overview','pointermove',{button:2,clientX:520});emit('overview','pointerup',{button:2,clientX:520});assert.equal(view().rangeSelected,true);
emit('overview','pointerdown',{button:2});emit('overview','pointercancel',{button:2});assert.equal(view().rangeSelected,true);
emit('mouse-help','pointerenter');assert.ok(element('tooltip').textContent.includes('Double-click outside selection'));assert.ok(element('tooltip').textContent.includes('Ctrl+wheel'));emit('mouse-help','pointerleave');assert.equal(element('tooltip').style.display,'none');
emit('mouse-help','focus');assert.equal(element('tooltip').style.display,'block');emit('mouse-help','blur');assert.equal(element('tooltip').style.display,'none');context.window.gpuProfiler.fit();
assert.equal(evalValue('aliveAt(249999).length'),0);assert.equal(evalValue('aliveAt(250000).length'),1);
assert.equal(evalValue('aliveAt(350000).length'),4);assert.equal(evalValue('aliveAt(425000).length'),1);assert.equal(evalValue('aliveAt(450000).length'),0);
assert.equal(evalValue('commandPhases(cmd.get(12),425000)'),'P');
assert.ok(evalValue('aliveText(350000)').includes('WAIT'));assert.ok(evalValue('aliveText(350000)').includes('[hidden type]'));
emit('commands','pointermove',{clientX:100,clientY:evalValue('rowOf.get(12).y+8')});assert.ok(element('tooltip').textContent.includes('Execute'));assert.ok(!element('tooltip').textContent.includes('Alive commands'));
const liveX=264+.425*524;emit('commands','pointermove',{clientX:liveX,clientY:10});
assert.ok(element('tooltip').textContent.includes('Alive commands'));assert.ok(element('tooltip').textContent.includes('Rabbit 02'));assert.ok(!element('tooltip').textContent.includes('Rabbit 03'));
const hover=element('tooltip').textContent;emit('commands','pointermove',{clientX:liveX,clientY:150});assert.equal(element('tooltip').textContent,hover);
for(const button of [1,2]){
 context.window.gpuProfiler.focus(12);const before=view();
 element('command-scroll').scrollTop=300;
 emit('commands','pointerdown',{button});emit('commands','pointermove',{clientX:550,clientY:-90});const after=view();
 assert.ok(after.lo<before.lo);assert.equal(after.hi-after.lo,before.hi-before.lo);
 assert.equal(element('command-scroll').scrollTop,400);
 emit('commands','pointerup',{button});emit('commands','pointermove',{clientX:560});assert.equal(view().lo,after.lo);
}
context.window.gpuProfiler.focus(12);let before=view();
emit('overview','pointerdown',{clientX:overviewX((before.lo+before.hi)/2)});
emit('overview','pointermove',{clientX:overviewX((before.lo+before.hi)/2+100000)});
let after=view();assert.ok(Math.abs(after.lo-before.lo-100000)<1e-6);assert.equal(after.hi-after.lo,before.hi-before.lo);emit('overview','pointerup');
context.window.gpuProfiler.focus(12);before=view();
emit('overview','pointerdown',{clientX:overviewX(before.lo)});emit('overview','pointermove',{clientX:overviewX(before.lo-50000)});
assert.ok(Math.abs(view().hi-before.hi)<1e-6);assert.ok(view().lo<before.lo);emit('overview','pointercancel');
emit('filter-options','change',{target:{value:'3',checked:false}});assert.equal(view().selected,null);
context.window.gpuProfiler.fit();
let prevented=false;before=view();emit('load','wheel',{deltaY:100,ctrlKey:false,preventDefault:()=>prevented=true});assert.equal(prevented,false);assert.equal(view().hi,before.hi);
emit('load','wheel',{deltaY:-100,ctrlKey:true,preventDefault:()=>prevented=true});assert.equal(prevented,true);assert.ok(view().hi-view().lo<before.hi-before.lo);context.window.gpuProfiler.fit();
const mixed=evalValue('binStats(D.metric_defs[0],300000)');assert.ok(mixed.mean>0&&mixed.mean<1);assert.equal(mixed.exact,false);
const rate=evalValue('binStats(D.metric_defs[9],400000)');assert.equal(rate.mean,2);
assert.ok(evalValue('statText(D.metric_defs[9],binStats(D.metric_defs[9],400000))').includes('2 accepted events'));
emit('load','pointermove',{clientX:264+.4*524,clientY:9*47+10});
assert.ok(element('tooltip').textContent.includes('Rendered bin'));assert.ok(element('tooltip').textContent.includes('2 accepted events'));
context.window.gpuProfiler.focus(0); // Narrow enough for an exact-state binary rendering.
evalValue('fitRange(300000,300064)');assert.equal(evalValue('binStats(D.metric_defs[0],300001).exact'),true);
console.log('UI checks passed: type-only filters, empty/default, unselected overview first-drag selection and reset, label/plot hover separation, all-type alive lifetimes through DMA/cache commit, middle/right X/Y pan, wheel scroll/Ctrl zoom, overview move/resize, release/cancel, aggregate and exact-state hover.');
if(process.argv[2]){
 const session=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));elements.clear();
 const second={...context,window:{devicePixelRatio:1}};vm.createContext(second);
 vm.runInContext(html.split('<script>')[1].split('</script>')[0].replace('/*PROFILE_DATA*/null',JSON.stringify(session)),second,{timeout:10000});
 const run=s=>vm.runInContext(s,second,{timeout:5000});
 assert.equal(run('cmd.size'),9);assert.equal(run('rowOf.size'),6);
 for(const id of [2,5,8]){second.window.gpuProfiler.focus(id);assert.equal(second.window.gpuProfiler.getView().selected,id);assert.ok(element('detail').innerHTML.includes('command #'+id));}
 assert.equal(run('duration(1000)'),'1.0000 ms');
 assert.ok(run('aliveText(72)').includes('CB2'));assert.ok(!run('aliveText(72)').includes('CB1'));
 assert.equal(run("rows[0].spans.some(s=>s.start<submissions.get(1).end&&s.end>submissions.get(1).start)"),true);
 console.log('Session viewer passed: A/B/A repeated-PC selection, CB-specific alive, CPU overlap and variable clock.');
}
