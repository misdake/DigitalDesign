// Run with: node systems/cpu-v3-tang-nano-20k/tests/debugger-ui.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const html = fs.readFileSync(path.join(__dirname, '../src/bin/cpu-v3-dbg-ui.html'), 'utf8');
const script = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));
new vm.Script(script); // Syntax-check the complete client without running DOM code.
const context = vm.createContext({
  state: { types: [
    { name: 'Inner', size: 2, fields: [
      { name: 'x', offset: 0, ty: 'u16' }, { name: 'y', offset: 1, ty: 'i16' }
    ] },
    { name: 'Outer', size: 4, fields: [
      { name: 'child', offset: 0, ty: 'Inner' }, { name: 'flags', offset: 2, ty: 'Buf<u16, 2>' }
    ] }
  ] },
  hx: n => n.toString(16).padStart(4, '0')
});
vm.runInContext(script.slice(script.indexOf('function varRow('), script.indexOf('async function renderMem')), context);
const rendered = context.varRow({ name: 'p', ty: 'Outer', value: { addr: 64, words: [7, 65529, 8, 9] } });
for (const expected of ['child', 'flags', '>x<', '>y<', '-7 (fff9)', '[0]', '[1]', 'data-addr="65"', 'data-addr="67"']) {
  assert.ok(rendered.includes(expected), `${expected} missing from ${rendered}`);
}
assert.ok(rendered.includes('<details class="aggregate" open>'));
assert.ok(context.varRow({ name: '<x>', ty: 'u16', value: { reg: 1, word: 42 } }).includes('&lt;x&gt;'));
assert.ok(context.fmtVal({ ty: 'i16', value: { addr: 0, words: [65535] } }).includes('-1 (ffff)'));
assert.equal(context.fmtVal({ ty: 'Inner', value: null }), '<i>ssa</i>');
console.log('Debugger aggregate rendering passed.');
