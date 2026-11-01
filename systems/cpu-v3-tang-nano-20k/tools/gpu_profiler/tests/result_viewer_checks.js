const fs=require('fs'),path=require('path'),vm=require('vm'),assert=require('assert');
const template=fs.readFileSync(path.join(__dirname,'../viewer.html'),'utf8').split('<script>')[1].split('</script>')[0];
function element(tag){return{tag,style:{},children:[],append(...children){this.children.push(...children);},replaceChildren(...children){this.children=children;}};}
for(const has_target of [true,false]){
 const header=element('header'),main=element('main');
 const data={schema:'digitaldesign.gpu-result',version:1,backend:'sim',complete:true,has_target,
  input_sha256:'1'.repeat(64),scope:'Final result only',final:{color:{image:'data:image/png;base64,eA=='}},
  outputs:[{role:'write-range',address:1024,bytes:512,file:'capture/range-1.bin',sha256:'2'.repeat(64)}]};
 const context={document:{querySelector:s=>s==='header'?header:main,createElement:element},window:{}};
 vm.runInNewContext(template.replace('/*PROFILE_DATA*/null',JSON.stringify(data)),context,{timeout:1000});
 assert.strictEqual(context.window.gpuResult.data.backend,'sim');
 assert.strictEqual(context.window.gpuProfiler,undefined);
 assert.strictEqual(main.children.some(n=>n.tag==='img'),has_target);
 assert.ok(main.children.some(n=>n.tag==='table'));
 assert.ok(!main.children.some(n=>n.tag==='canvas'));
}
console.log('Result viewer checks passed: final framebuffer or raw-buffer results, no profiling/cycle renderer.');
