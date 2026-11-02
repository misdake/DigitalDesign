// Bounded application-logic checks with a minimal DOM/canvas adapter.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),path=require('node:path');
const elements=new Map(),noop=()=>{};
function element(id){if(elements.has(id))return elements.get(id);const listeners={},capture=new Set();
 const draws=[];const ctx=new Proxy({measureText:s=>({width:s.length*6}),fillRect:(...args)=>draws.push(args)},{get:(o,k)=>k in o?o[k]:noop,set:(o,k,v)=>(o[k]=v,true)});
 const el={id,listeners,draws,dataset:{},style:{},clientWidth:800,offsetHeight:70,scrollTop:0,innerHTML:'',textContent:'',value:'',
 removeAttribute(name){delete this[name];},getAttribute(name){return this[name]??null;},
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
// Inspect private renderer state from its actual conditional lexical scope.
vm.runInContext(script.replace('const $=','globalThis.viewerTestEval=code=>eval(code);\nconst $='),context,{timeout:10000});
function emit(id,type,args={}){for(const fn of element(id).listeners[type]??[])fn({type,button:0,pointerId:7,clientX:500,clientY:10,preventDefault:noop,...args});}
function view(){return context.window.gpuProfiler.getView();}
function evalValue(s){return context.viewerTestEval(s);}
assert.equal(element('frame-preview').hidden,false);assert.equal(element('frame-wrap').hidden,true);assert.ok(element('frame-caption').textContent.includes('not saved'));
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
evalValue('fitRange(200000,500000)');emit('overview','pointerdown',{button:2});emit('overview','pointerup',{button:2});assert.equal(view().rangeSelected,true);
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
for(const button of [1]){
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
emit('load','pointermove',{clientX:264+.4*524,clientY:evalValue("loadMetrics.findIndex(d=>d.key===D.metric_defs[9].key)")*47+10});
assert.ok(element('tooltip').textContent.includes('Rendered bin'));assert.ok(element('tooltip').textContent.includes('2 accepted events'));
context.window.gpuProfiler.focus(0); // Narrow enough for an exact-state binary rendering.
evalValue('fitRange(300000,300064)');assert.equal(evalValue('binStats(D.metric_defs[0],300001).exact'),true);
// Legacy full-capture reference does not become command accounting.
const traffic={read_wire_bytes:8,write_wire_bytes:24,start:0,end:256,average_wire_bytes_per_second:17000000,average_read_bytes_per_second:1000000,average_write_bytes_per_second:16000000,peak_wire_bytes_per_second:32000000,peak_window_cycles:256,issued_transactions:1,completed_transactions:1,data_beats:4,wire_bytes:32,effective_bytes:32,request_backpressure_link_cycles:2,outstanding_without_data_link_cycles:3,write_terminal_wait_link_cycles:1,utilization:.1,links:[],sources:[]};
evalValue('D.memory='+JSON.stringify({available:true,complete:true,clock_hz:54000000,summary:traffic,commands:{0:{...traffic,average_wire_bytes_per_second:33000000}}}));
context.window.gpuProfiler.focus(0);
assert.ok(element('gpu-memory-summary').innerHTML.includes('17.00 MB/s'));
assert.ok(!element('gpu-memory-summary').innerHTML.includes('33.00 MB/s'));
evalValue('selectRow(null)');assert.ok(element('gpu-memory-summary').innerHTML.includes('Full capture'));assert.ok(element('gpu-memory-summary').innerHTML.includes('17.00 MB/s'));
assert.ok(!element('gpu-memory-summary').innerHTML.includes('NaN'));
// Inline image selection uses sentinels in this DOM-only adapter (no image decoding).
const finalFrame={cycle:1000000,target_base_bytes:4456448,target_epoch:1,format:1,src:'data:image/png;base64,FINAL'};
evalValue('D.images='+JSON.stringify({available:true,final:finalFrame,commands:{0:{...finalFrame,cycle:400000,src:'data:image/png;base64,COMMAND'}}}));
evalValue('selectRow(null)');assert.equal(element('frame-label').textContent,'Final output');assert.equal(element('frame-image').src,finalFrame.src);
context.window.gpuProfiler.focus(0);assert.ok(element('frame-label').textContent.includes('Command #0'));assert.ok(element('frame-image').src.endsWith('COMMAND'));
let menuPrevented=false;emit('load','contextmenu',{preventDefault:()=>menuPrevented=true});assert.equal(menuPrevented,false);
emit('overview','contextmenu',{preventDefault:()=>menuPrevented=true});assert.equal(menuPrevented,false);
emit('commands','contextmenu',{button:2,preventDefault:()=>menuPrevented=true});assert.equal(menuPrevented,true);assert.equal(view().selected,null);assert.equal(view().rangeSelected,false);assert.equal(element('frame-image').src,finalFrame.src);
console.log('UI checks passed: type-only filters, empty/default, unselected overview first-drag selection and reset, label/plot hover separation, all-type alive lifetimes through DMA/cache commit, middle X/Y pan, wheel scroll/Ctrl zoom, overview move/resize, release/cancel, aggregate and exact-state hover.');
if(process.argv[2]){
 const session=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));elements.clear();
 const second={...context,window:{devicePixelRatio:1}};vm.createContext(second);
 vm.runInContext(html.split('<script>')[1].split('</script>')[0].replace('/*PROFILE_DATA*/null',JSON.stringify(session)).replace('const $=','globalThis.viewerTestEval=code=>eval(code);\nconst $='),second,{timeout:10000});
 const run=s=>second.viewerTestEval(s);
 assert.equal(run('cmd.size'),9);assert.equal(run('rowOf.size'),6);
 for(const id of [2,5,8]){second.window.gpuProfiler.focus(id);assert.equal(second.window.gpuProfiler.getView().selected,id);assert.ok(element('detail').innerHTML.includes('command #'+id));}
 assert.equal(run('duration(1000)'),'1.0000 ms');
 assert.ok(run('aliveText(72)').includes('CB2'));assert.ok(!run('aliveText(72)').includes('CB1'));
 assert.equal(run("rows[0].spans.some(s=>s.start<submissions.get(1).end&&s.end>submissions.get(1).start)"),true);
 console.log('Session viewer passed: A/B/A repeated-PC selection, CB-specific alive, CPU overlap and variable clock.');
}

// Active-range statistics must not substitute command-local accounting.
assert.ok(html.includes('GPU Activity &amp; Memory Throughput'));
assert.ok(!html.includes('id="memory"'));
evalValue(`D.memory={available:true,complete:true,clock_hz:54000000,summary:{start:0,end:1000000,read_wire_bytes:128,write_wire_bytes:64,average_read_bytes_per_second:6912,average_write_bytes_per_second:3456,average_wire_bytes_per_second:10368,peak_window_cycles:256,peak_wire_bytes_per_second:null}};memoryOverview();`);
const overviewTraffic=element('gpu-memory-summary').innerHTML;
assert.ok(overviewTraffic.includes('128 B')&&overviewTraffic.includes('64 B')&&overviewTraffic.includes('Excludes CPU and Display'));
evalValue('selected=D.commands.find(c=>c.opcode===3).id;detail();');
assert.equal(element('gpu-memory-summary').innerHTML,overviewTraffic);
// Even a legacy image entry must not expose an empty DRAW's early completion as a picture.
evalValue(`D.images={available:true,commands:{[selected]:{src:'unsafe-early-image'}},final:{src:'final'}};frameDetail(cmd.get(selected));`);
assert.equal(element('frame-preview').hidden,false);assert.equal(element('frame-wrap').hidden,true);assert.ok(element('frame-caption').textContent.includes('no quads'));
for(const id of ['frame-image','frame-large'])assert.equal(element(id).getAttribute('src'),null,'empty DRAW clears stale image attribute');
evalValue('D.images={available:false,reason:"missing raw evidence"};frameDetail(null);');
assert.equal(element('frame-wrap').hidden,true);assert.ok(element('frame-caption').textContent.includes('missing raw evidence'));
for(const id of ['frame-image','frame-large'])assert.equal(element(id).getAttribute('src'),null,'unavailable clears stale image attribute');
evalValue('D.images='+JSON.stringify({available:true,commands:{},final:finalFrame})+';frameDetail(null);');
assert.equal(element('frame-wrap').hidden,false);
for(const id of ['frame-image','frame-large'])assert.equal(element(id).getAttribute('src'),finalFrame.src,'new image restores the current source');

// Exact memory timing uses the existing renderer, axis, zoom and cursor logic.
function memorySession(profile){
 elements.clear();const c={...context,window:{devicePixelRatio:1}};vm.createContext(c);
 const code=html.split('<script>')[1].split('</script>')[0].replace('/*PROFILE_DATA*/null',JSON.stringify(profile));
 vm.runInContext(code.replace('const $=','globalThis.viewerTestEval=code=>eval(code);\nconst $='),c,{timeout:20000});
 return {run:s=>c.viewerTestEval(s),view:()=>c.window.gpuProfiler.getView()};
}
const memoryFixture=JSON.parse(JSON.stringify(data));
memoryFixture.memory={available:true,complete:true,clock_hz:54000000,
 summary:{start:0,end:1000000,read_wire_bytes:64,write_wire_bytes:32,average_read_bytes_per_second:3456,
 average_write_bytes_per_second:1728,average_wire_bytes_per_second:5184,peak_window_cycles:256,peak_wire_bytes_per_second:null},
 transactions:[{start:10,end:20,port:0,expected_beats:16},{start:12,end:50,port:2,expected_beats:4},{start:600000,end:600005,port:1,expected_beats:4}],
 timeline:{available:true,version:1,start:0,end:1000000,
 data:[[14,8,8],[15,8,8],[16,8,0],[17,8,0],[18,0,8],[19,0,8],[600001,8,0],[600002,8,0],[600003,8,0],[600004,8,0]],
 states:{outstanding:[[0,0],[10,1],[12,2],[20,1],[50,0],[600000,1],[600005,0]],
 waiting:[[0,0],[10,1],[12,2],[14,0],[16,1],[50,0],[600000,1],[600001,0]],terminal_wait:[[0,0],[20,1],[50,0]]},
 backpressure:{available:true,start:5,end:1000000,track:[[5,0],[8,1],[10,0]]}}};
let mem=memorySession(memoryFixture);
// Both inspector groups use the same active view range, independent of hover bins.
const groups=['inspect-section','legend-section','bin-section','memory-section'];
for(const id of groups)element(id).open=false;
mem.run('fitRange(14.25,78.25)');
assert.ok(Number.isInteger(mem.view().lo)&&Number.isInteger(mem.view().hi));
assert.equal(element('cursor').dataset.rangeStart,'14');
assert.equal(element('cursor').dataset.rangeEnd,'78');
assert.equal(element('gpu-memory-summary').dataset.rangeStart,'14');
assert.equal(element('gpu-memory-summary').dataset.rangeEnd,'78');
let selectedStats=mem.run('memoryRangeStats(14.25,17)');
assert.equal(selectedStats.read,16);assert.equal(selectedStats.write,8);
assert.equal(selectedStats.averageRead,16*54000000/2.75);
assert.equal(selectedStats.averageWrite,8*54000000/2.75);
assert.equal(selectedStats.peak,null,'no complete 256-cycle peak window');
assert.equal(mem.run('memoryRangeStats(20,20).averageTotal'),null);
assert.equal(mem.run('memoryRangeStats(20,20).read'),0);
assert.equal(mem.run('memoryRangeStats(20,30).averageTotal'),0);
assert.equal(mem.run('memoryRangeStats(-1,30).complete'),false);
assert.equal(mem.run('memoryRangeStats(-1,30).averageTotal'),null);
const inspectorRange=element('cursor').innerHTML;
mem.run('showCursor(15.75)');assert.equal(element('cursor').innerHTML,inspectorRange,'hover must not replace active-range statistics');
mem.run('selected=D.commands.find(c=>c.opcode===3).id;detail();render();');
for(const id of groups)assert.equal(element(id).open,false,'redraw preserves collapse state');
mem.run('fitRange(D.start,D.end)');
assert.equal(element('cursor').dataset.rangeStart,'0');
assert.equal(element('gpu-memory-summary').dataset.rangeEnd,'1000000');
assert.equal(mem.run('memoryRangeStats(lo,hi).read'),64);
assert.equal(mem.run('memoryRangeStats(lo,hi).write'),32);
for(const id of groups)assert.equal(element(id).open,false,'clear selection preserves collapse state');
// Independent raw-event enumeration for fractional windows and strict endpoints.
// Clicking a DRAW selects its full asynchronous lifetime, not just issue/release.
mem.run('suppressClick=false');
emit('commands','click',{clientX:100,clientY:mem.run('rowOf.get(12).y+8')});
assert.equal(mem.run('activeRange().source'),'draw');
assert.equal(mem.run('activeRange().a'),250000);assert.equal(mem.run('activeRange().b'),450000);
assert.equal(element('cursor').dataset.rangeStart,'250000');
assert.equal(element('gpu-memory-summary').dataset.rangeEnd,'450000');
emit('focus','click');
assert.ok(mem.view().lo<250000&&mem.view().hi>450000,'focus keeps viewport padding');
assert.equal(mem.run('activeRange().a'),250000);assert.equal(mem.run('activeRange().b'),450000);
emit('load','wheel',{clientX:500,clientY:10,deltaY:-100,ctrlKey:true});
assert.equal(mem.run('activeRange().source'),'draw');assert.equal(mem.run('activeRange().b'),450000);
mem.run('jobs.get(1).start=250000.25;mesh.get(1).last_quad.end=450000.75;selectRow(rowOf.get(12));');
assert.equal(mem.run('activeRange().a'),250000);assert.equal(mem.run('activeRange().b'),450001);
mem.run('jobs.get(1).start=250000;mesh.get(1).last_quad.end=450000;fitRange(0,1000000,true)');
emit('overview','pointerdown',{clientX:overviewX(0)});emit('overview','pointermove',{clientX:overviewX(500000)});emit('overview','pointerup');
assert.equal(mem.run('activeRange().source'),'range');assert.equal(mem.run('activeRange().b'),500000);
assert.equal(element('cursor').dataset.rangeEnd,element('gpu-memory-summary').dataset.rangeEnd);
mem.run('selectRow(rowOf.get(0))');assert.equal(mem.run('activeRange().source'),'range','non-DRAW retains time range');
emit('commands','contextmenu');assert.equal(mem.run('activeRange().source'),'frame');assert.equal(mem.run('activeRange().b'),1000000);
const peakEvents=memoryFixture.memory.timeline.data;
for(const [a,b,width]of [[0,1000000,256],[14.25,300.25,256],[14,18,4],[14.25,18.25,4],[15,19,4]]){
 let maximum=0;
 const starts=[a,b-width,...peakEvents.map(r=>r[0])].filter(x=>a<=x&&x+width<=b);
 for(const start of starts)maximum=Math.max(maximum,peakEvents.filter(r=>start<=r[0]&&r[0]<start+width).reduce((n,r)=>n+r[1]+r[2],0));
 assert.equal(mem.run(`memoryView.peakBytes(${a},${b},${width})`),maximum);
}

const requested=mem.run('memoryView.interval(0,13)');
assert.equal(requested.read,0);assert.equal(requested.write,0);assert.equal(requested.requests,2);
assert.equal(requested.requestedRead,128);assert.equal(requested.requestedWrite,32);
assert.equal(mem.run('memoryView.interval(14,16).read'),16);assert.equal(mem.run('memoryView.interval(14,15).read'),8);
const stalled=mem.run('memoryView.interval(20,50)');
assert.equal(stalled.read+stalled.write,0);assert.equal(stalled.outstanding.mean,1);
assert.equal(stalled.waiting.cycles,30);assert.equal(stalled.terminal.cycles,30);
const idle=mem.run('memoryView.interval(70,80)');assert.equal(idle.read+idle.write,0);assert.equal(idle.outstanding.mean,0);
assert.equal(mem.run('memoryView.interval(0,4).backpressure'),null);
assert.equal(mem.run('memoryView.interval(20,20).empty'),true);
for(const [a,b]of [[0,1000000],[0,500000],[13.5,19.5],[14,15],[19,20],[20,50],[599999.3,600004.2]]){
 for(const n of [1,2,7,53,197]){
  for(const [key,column]of [['mem_read',1],['mem_write',2]]){
   const values=Array.from(mem.run(`aggregate(memoryView.tracks.${key},'bytes',${a},${b},${n}).mean`));
   const rows=memoryFixture.memory.timeline.data,expected=rows.filter(r=>a<=r[0]&&r[0]<b).reduce((s,r)=>s+r[column],0);
   assert.equal(values.reduce((s,v)=>s+v,0),expected,'byte bins must conserve visible events');
   for(let k=0;k<n;k++){
    const begin=a+k*(b-a)/n,end=k+1===n?b:a+(k+1)*(b-a)/n;
    assert.equal(values[k],rows.filter(r=>begin<=r[0]&&r[0]<end).reduce((s,r)=>s+r[column],0),`boundary bin ${k}`);
   }
  }
 }
}
// Actual canvas operations: four8B read bars and four8B write bars after zoom.
element('load').draws.length=0;mem.run('fitRange(8,72)');
const byteBars=element('load').draws.filter(r=>r[0]>=264&&r[1]>=0&&r[1]<94&&r[3]>0);
assert.equal(byteBars.length,8);assert.ok(byteBars.every(r=>r[3]===27));
const pointerX=264+(14.1-8)/64*524;
emit('load','pointermove',{clientX:pointerX,clientY:10});
assert.ok(element('tooltip').textContent.includes('[14.00, 15.00)'));
assert.ok(element('tooltip').textContent.includes('8 actual bytes'));
assert.ok(element('tooltip').textContent.includes('Actual read 8 B · write 8 B'));
// Binary detail may show one cycle, but the appended memory readout uses the
// same rendered bin as the byte histograms, not a different one-cycle window.
mem.run('fitRange(0,512)');emit('load','pointermove',{clientX:264+16.1/512*524,clientY:5*47+10});
assert.ok(element('tooltip').textContent.includes('Memory shared bin'));
assert.ok(element('tooltip').textContent.includes('Actual read 24 B · write 8 B'));
mem.run('fitRange(0,1000)');const zoomBefore=mem.view();
emit('load','wheel',{clientX:500,clientY:10,deltaY:-100,ctrlKey:true});
assert.ok(mem.view().hi-mem.view().lo<zoomBefore.hi-zoomBefore.lo);
mem.run('fitRange(0,1000000)');
emit('overview','pointerdown',{clientX:overviewX(0)});emit('overview','pointermove',{clientX:overviewX(500000)});emit('overview','pointerup');
assert.equal(mem.view().rangeSelected,true);assert.equal(mem.view().hi,500000);
assert.ok(element('gpu-memory-summary').innerHTML.includes('Actual read 32 B · write 32 B'));
assert.ok(element('gpu-memory-summary').innerHTML.includes('64 B'),'full-capture overview remains distinct');
const legacy=JSON.parse(JSON.stringify(memoryFixture));delete legacy.memory.timeline;
mem=memorySession(legacy);assert.equal(mem.run('memoryView.definitions[0].available'),false);
assert.equal(mem.run('binStats(memoryView.definitions[0],100).available'),false);
assert.ok(mem.run('statText(memoryView.definitions[0],binStats(memoryView.definitions[0],100))').includes('Unavailable'));
assert.ok(!element('load').draws.some(r=>r[0]>=264&&r[1]>=0&&r[1]<94&&r[3]>0),'missing timing must not draw zero/estimated traffic');
console.log('Memory viewer passed: real histogram draw calls, fractional-bin conservation, accepted vs transferred bytes, zero-data outstanding/ACK wait, idle, coverage unknown, old-profile unavailable, shared hover/Ctrl zoom/selection.');
if(process.argv[3]){
 const actual=JSON.parse(fs.readFileSync(process.argv[3],'utf8')),m=actual.memory;
 mem=memorySession(actual);
 if(actual.images?.available){assert.equal(element('frame-preview').hidden,false);assert.equal(element('frame-wrap').hidden,false);assert.equal(element('frame-image').src,actual.images.final.src);}
 if(actual.images?.final.preview?.encoding==='srgb'){
  assert.ok(element('frame-caption').textContent.includes('Display1 sRGB preview'));
  assert.ok(!html.includes('filter:'),'saved display PNG is not gamma-filtered by the viewer');
 }
 const selectedDraw=actual.commands.find(c=>c.opcode===3&&actual.meshlets.find(m=>m.id===c.meshlet)?.quads>0);
 if(selectedDraw){
  const meshlet=actual.meshlets.find(m=>m.id===selectedDraw.meshlet),dma=actual.jobs.find(j=>j.id===meshlet.dma);
  const expectedStart=Math.floor(Math.min(selectedDraw.start,dma?.start??selectedDraw.start));
  const expectedEnd=Math.ceil(Math.max(selectedDraw.complete_cycle??selectedDraw.end,meshlet.last_quad?.end??0,...selectedDraw.jobs.map(id=>actual.jobs.find(j=>j.id===id)?.end??0)));
  mem.run(`selectRow(rowOf.get(${selectedDraw.id}))`);
  assert.equal(element('cursor').dataset.rangeStart,String(expectedStart));assert.equal(element('gpu-memory-summary').dataset.rangeEnd,String(expectedEnd));
  assert.equal(mem.run('activeRange().source'),'draw');
 }

 for(const [a,b,n]of [[0,actual.end,173],[0,actual.end,541],[123.25,actual.end-99.5,97],[1000,2000,37]]){
  for(const [key,column,total]of [['mem_read',1,m.summary.read_wire_bytes],['mem_write',2,m.summary.write_wire_bytes]]){
   const bins=Array.from(mem.run(`aggregate(memoryView.tracks.${key},'bytes',${a},${b},${n}).mean`));
   const expected=m.timeline.data.filter(r=>a<=r[0]&&r[0]<b).reduce((s,r)=>s+r[column],0);
   assert.equal(bins.reduce((s,v)=>s+v,0),expected);
   if(a===0&&b===actual.end)assert.equal(expected,total);
   for(const k of [0,Math.floor(n/2),n-1]){
    const begin=a+k*(b-a)/n,end=k+1===n?b:a+(k+1)*(b-a)/n;
    assert.equal(bins[k],m.timeline.data.filter(r=>begin<=r[0]&&r[0]<end).reduce((s,r)=>s+r[column],0));
   }
  }
 }

 for(const [a,b]of [[1000.25,2000.75],[actual.memory.timeline.end-10,actual.end],[0,actual.end]]){
  mem.run(`lo=${a};hi=${b};rangeSelected=true;statsSource='view';render();`);
  const stats=mem.run(`memoryRangeStats(${a},${b})`);
  const c=stats.coverage;
  const rows=m.timeline.data.filter(r=>Math.max(a,m.timeline.start)<=r[0]&&r[0]<Math.min(b,m.timeline.end));
  assert.equal(stats.read,rows.reduce((n,r)=>n+r[1],0));assert.equal(stats.write,rows.reduce((n,r)=>n+r[2],0));
  assert.equal(element('cursor').dataset.rangeStart,element('gpu-memory-summary').dataset.rangeStart);
  assert.equal(element('cursor').dataset.rangeEnd,element('gpu-memory-summary').dataset.rangeEnd);
  assert.equal(stats.complete,!c.partial);
  if(c.partial)assert.equal(stats.averageTotal,null);
 }
 mem.run('fitRange(D.start,D.end)');
 const whole=mem.run(`memoryView.interval(0,${actual.end})`);
 assert.equal(whole.waiting.cycles,m.summary.outstanding_without_data_link_cycles);
 assert.equal(whole.terminal.cycles,m.summary.write_terminal_wait_link_cycles);
 const bp=mem.run(`memoryView.interval(${actual.sample_start},${actual.sample_end}).backpressure`);
 assert.equal(bp.cycles,m.summary.request_backpressure_link_cycles);
 assert.equal(whole.backpressure.coverage.partial,true,'unobserved valid/ready slots remain unknown');
 assert.deepEqual(Array.from([whole.backpressure.coverage.a,whole.backpressure.coverage.b]),[actual.sample_start,m.timeline.backpressure.end]);
 assert.equal(whole.coverage.partial,actual.end>m.timeline.end);
 console.log(JSON.stringify({actual_memory_viewer:'PASS',input_sha256:actual.provenance.input_sha256,commands:actual.commands.length,
 read_bytes:whole.read,write_bytes:whole.write,waiting_link_cycles:whole.waiting.cycles,write_terminal_wait:whole.terminal.cycles,
 backpressure_link_cycles:bp.cycles,memory_coverage:[whole.coverage.a,whole.coverage.b],backpressure_coverage:[bp.coverage.a,bp.coverage.b]}));
}
