// Capture upright static cameras through the existing Rust/Wasm inspection API.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync} from 'node:fs';
import {readFile, mkdir, mkdtemp, open, rename, realpath, rm} from 'node:fs/promises';
import path from 'node:path';
import {pathToFileURL} from 'node:url';

const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) {
  assert.ok(
    ['--source-root', '--cameras', '--output', '--site', '--chrome'].includes(process.argv[i]) &&
      process.argv[i + 1],
    'Usage: node rust_reference.mjs --source-root CHECKOUT --cameras CAMERAS.json --output FRESH_DIRECTORY [--site SITE] [--chrome EXECUTABLE]'
  );
  assert.ok(!args.has(process.argv[i]), 'Repeated argument: ' + process.argv[i]);
  args.set(process.argv[i], path.resolve(process.argv[i + 1]));
}
for (const name of ['--source-root', '--cameras', '--output'])
  assert.ok(args.has(name), 'Required: ' + name);
const source = args.get('--source-root');
const site = args.get('--site') ?? path.join(source, 'build/rust/site-extra-maps');
const output = args.get('--output');
const relativeOutput = path.relative(source, output);
assert.ok(
  relativeOutput.startsWith('..' + path.sep) || path.isAbsolute(relativeOutput),
  'Output must be outside the read-only source checkout'
);
const cameraBytes = await readFile(args.get('--cameras'));
const cameras = JSON.parse(cameraBytes);
assert.ok(
  Array.isArray(cameras) && cameras.length >= 1 && cameras.length <= 16,
  'Expected 1..16 static cameras'
);
for (const [index, camera] of cameras.entries()) {
  assert.ok(camera && typeof camera === 'object', 'Camera ' + (index + 1) + ' must be an object');
  assert.ok(
    !('roll' in camera) && !('up' in camera),
    'Camera roll/up are unsupported; use upright eye/target cameras'
  );
  for (const key of ['eye', 'target'])
    assert.ok(
      Array.isArray(camera[key]) &&
        camera[key].length === 3 &&
        camera[key].every((v) => typeof v === 'number' && Number.isFinite(v)),
      key + ' requires three finite numbers'
    );
  assert.ok(
    camera.label === undefined || typeof camera.label === 'string',
    'Camera label must be a string'
  );
  const direction = camera.target.map((v, i) => v - camera.eye[i]);
  const length = Math.hypot(...direction);
  assert.ok(length > 1e-8, 'Camera eye and target must differ');
  assert.ok(
    Math.abs(Math.asin(direction[1] / length)) <= 1.5,
    'Camera pitch exceeds the existing Rust camera limit of 1.5 radians'
  );
}
const executable =
  args.get('--chrome') ??
  process.env.CHROME ??
  [
    ...[process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA]
      .filter(Boolean)
      .map((base) => path.join(base, 'Google', 'Chrome', 'Application', 'chrome.exe')),
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
    '/usr/bin/google-chrome',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
  ].find(existsSync);
assert.ok(executable && existsSync(executable), 'Provide --chrome or CHROME pointing to Chromium');
const hash = (data) => createHash('sha256').update(data).digest('hex');
const basename = (name) => path.posix.basename(name.replaceAll('\\', '/'));
const referenceFiles = [
  'pkg/tankracer_bg.wasm',
  'pkg/tankracer.js',
  'main.js',
  'assets/maps/ENGLAND.trmap'
];
const files = {};
for (const name of referenceFiles) files[name] = hash(await readFile(path.join(site, name)));
const {createWebServer} = await import(pathToFileURL(path.join(source, 'tools/serve.mjs')));
await mkdir(path.dirname(output), {recursive: true});
await mkdir(output);
const errors = [];
const report = {
  format: 'tankracer.rust-static-reference.v1',
  passed: false,
  source,
  site,
  cameras_sha256: hash(cameraBytes),
  cameras_file: basename(args.get('--cameras')),
  files,
  width: 400,
  height: 240,
  cases: [],
  errors,
  scope:
    'Existing Rust/Wasm renderer, upright static inspection world, camera-only API; source-world Z reflected once. No simulation updates. Visual reference rather than a bit-exact color oracle for the quantized FPGA renderer.'
};
let server,
  chrome,
  closed,
  profile,
  socket,
  spawnError,
  sequence = 0;
const pending = new Map();
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
async function wait(fn, label, timeout = 90000) {
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    if (spawnError) throw spawnError;
    if (chrome && chrome.exitCode !== null) throw Error('Owned Chromium exited before ' + label);
    const result = await fn();
    if (result) return result;
    await delay(100);
  }
  throw Error('Timed out: ' + label);
}
function send(method, params = {}) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(Error('CDP timed out: ' + method));
    }, 45000);
    pending.set(id, {
      timer,
      resolve: (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      reject: (error) => {
        clearTimeout(timer);
        reject(error);
      }
    });
    socket.send(JSON.stringify({id, method, params}));
  });
}
async function evaluate(body) {
  const result = await send('Runtime.evaluate', {
    expression: `(async()=>{${body}})()`,
    awaitPromise: true,
    returnByValue: true
  });
  if (result.exceptionDetails)
    throw Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
  return result.result.value;
}
async function durable(file, bytes) {
  const temporary = file + '.tmp-' + process.pid;
  const handle = await open(temporary, 'wx');
  try {
    await handle.writeFile(bytes);
    await handle.sync();
  } finally {
    await handle.close();
  }
  await rename(temporary, file);
  const actual = await readFile(file);
  assert.equal(actual.length, bytes.length);
  assert.equal(hash(actual), hash(bytes));
  if (file.endsWith('.json')) assert.ok(!actual.includes(0), 'JSON contains NUL');
}
try {
  server = createWebServer(site);
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const base = `http://127.0.0.1:${server.address().port}`;
  profile = await mkdtemp(path.join(output, 'chrome-'));
  chrome = spawn(
    executable,
    [
      '--headless=new',
      '--use-angle=swiftshader',
      '--enable-unsafe-swiftshader',
      '--no-first-run',
      '--no-default-browser-check',
      '--disable-background-networking',
      `--user-data-dir=${profile}`,
      '--remote-debugging-port=0',
      '--window-size=800,600',
      'about:blank'
    ],
    {stdio: 'ignore', windowsHide: true}
  );
  chrome.once('error', (error) => {
    spawnError = error;
  });
  closed = new Promise((resolve) => chrome.once('close', resolve));
  const port = await wait(async () => {
    try {
      return (await readFile(path.join(profile, 'DevToolsActivePort'), 'utf8')).split('\n')[0];
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      return false;
    }
  }, 'headless debugging port');
  const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) =>
    response.json()
  );
  const page = targets.find((target) => target.type === 'page');
  assert.ok(page, 'Owned headless browser must expose a page');
  socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    socket.onopen = resolve;
    socket.onerror = reject;
  });
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.id) {
      const item = pending.get(message.id);
      if (!item) return;
      pending.delete(message.id);
      if (message.error) item.reject(Error(JSON.stringify(message.error)));
      else item.resolve(message.result);
    } else if (message.method === 'Runtime.exceptionThrown')
      errors.push(message.params.exceptionDetails.text);
  };
  socket.onclose = () => {
    for (const item of pending.values()) item.reject(Error('Owned CDP connection closed'));
    pending.clear();
  };
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Page.navigate', {url: base + '/?inspect'});
  await wait(
    () =>
      evaluate(
        'if(document.body?.dataset.error)throw Error(document.body.dataset.error);return document.body?.dataset.ready === "true";'
      ),
    'Rust static world'
  );
  await evaluate(
    'const m=await import("/main.js");m.stop();document.getElementById("game").width=400;document.getElementById("game").height=240;return true;'
  );
  let initialState;
  for (const [index, camera] of cameras.entries()) {
    const capture = await evaluate(`
      const m=await import('/main.js'), app=m.app;
      if(!app.inspection_active())throw Error('Expected static inspection');
      const state_before=Array.from(app.recording_state());
      let forward=Array.from(app.camera_forward());
      app.look(-Math.atan2(forward[0],-forward[2])/.003,Math.asin(Math.max(-1,Math.min(1,forward[1])))/.003);
      const eye=${JSON.stringify(camera.eye)},target=${JSON.stringify(camera.target)};
      eye[2]=-eye[2];target[2]=-target[2];
      for(let i=0;i<512;i++){
        const current=Array.from(app.camera_position()),d=eye.map((v,j)=>v-current[j]),n=Math.hypot(...d);
        if(n<.00005)break;
        app.move_camera(Math.min(n/45,.1),d[0],d[1],-d[2],false);
      }
      const position=Array.from(app.camera_position());
      if(Math.hypot(...position.map((v,i)=>v-eye[i]))>.0001)throw Error('Camera position mismatch');
      const direction=target.map((v,i)=>v-position[i]),length=Math.hypot(...direction);
      app.look(Math.atan2(direction[0],-direction[2])/.003,-Math.asin(direction[1]/length)/.003);
      forward=Array.from(app.camera_forward());
      if(forward.reduce((v,x,i)=>v+x*direction[i]/length,0)<.999999)throw Error('Camera direction mismatch');
      app.render();app.check_graphics?.();
      const canvas=document.getElementById('game'),gl=canvas.getContext('webgl2');gl.finish();
      const pixels=new Uint8Array(400*240*4);gl.readPixels(0,0,400,240,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
      const error=gl.getError();if(error)throw Error('WebGL error '+error);
      let bytes='';for(let i=0;i<pixels.length;i+=8192)bytes+=String.fromCharCode(...pixels.subarray(i,i+8192));
      const state_after=Array.from(app.recording_state());
      if(JSON.stringify(state_before)!==JSON.stringify(state_after))throw Error('Camera/render changed game state');
      return {position,forward,state_before,state_after,state_preserved:true,frame:JSON.parse(app.frame_stats()),scene:JSON.parse(app.scene_stats()),rgba:btoa(bytes)};
    `);
    initialState ??= capture.state_before;
    assert.deepEqual(
      capture.state_before,
      initialState,
      'Game state changed between static cameras'
    );
    const rgba = Buffer.from(capture.rgba, 'base64');
    delete capture.rgba;
    assert.equal(rgba.length, 400 * 240 * 4);
    const rgb = Buffer.alloc(400 * 240 * 3);
    for (let y = 0; y < 240; y++)
      for (let x = 0; x < 400; x++)
        for (let c = 0; c < 3; c++)
          rgb[(y * 400 + x) * 3 + c] = rgba[((239 - y) * 400 + x) * 4 + c];
    const stem = 'frame-' + String(index + 1).padStart(3, '0');
    const image = Buffer.concat([Buffer.from('P6\n400 240\n255\n'), rgb]);
    const pose = {
      frame: index + 1,
      label: camera.label ?? String(index + 1),
      camera,
      rust_eye: capture.position,
      rust_forward: capture.forward,
      coordinate_conversion: 'Source Z reflected once',
      state_preserved: true,
      state_before: capture.state_before,
      state_after: capture.state_after
    };
    await durable(path.join(output, stem + '.ppm'), image);
    await durable(
      path.join(output, stem + '.pose.json'),
      Buffer.from(JSON.stringify(pose, null, 2) + '\n')
    );
    await durable(
      path.join(output, stem + '.stats.json'),
      Buffer.from(JSON.stringify({frame: capture.frame, scene: capture.scene}, null, 2) + '\n')
    );
    report.cases.push({
      frame: index + 1,
      label: pose.label,
      camera,
      image: stem + '.ppm',
      pose: stem + '.pose.json',
      stats: stem + '.stats.json',
      image_sha256: hash(image),
      raw_rgba_sha256: hash(rgba),
      state_preserved: true,
      position: capture.position,
      forward: capture.forward
    });
    console.log(
      'Rust reference',
      index + 1,
      pose.label,
      JSON.stringify({
        position: capture.position,
        forward: capture.forward,
        draws: capture.frame.draws,
        triangles: capture.frame.triangles
      })
    );
  }
  for (const name of referenceFiles)
    assert.equal(
      hash(await readFile(path.join(site, name))),
      files[name],
      'Reference file changed during capture: ' + name
    );
  assert.deepEqual(errors, []);
  report.state_preserved_between_cameras = true;
  report.passed = true;
} catch (error) {
  report.error = error.stack;
  process.exitCode = 1;
  console.error(error.stack);
} finally {
  // Own only the browser/profile and listener created above; never attach to a user browser.
  if (socket?.readyState === WebSocket.OPEN)
    socket.send(JSON.stringify({id: ++sequence, method: 'Browser.close'}));
  if (chrome) {
    let timer;
    const graceful = await Promise.race([
      closed.then(() => true),
      new Promise((resolve) => {
        timer = setTimeout(() => resolve(false), 5000);
      })
    ]);
    clearTimeout(timer);
    if (!graceful) {
      chrome.kill();
      await closed;
    }
  }
  socket?.close();
  if (server) {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
  if (profile) {
    // Same containment check and retry policy as the source project's browser test.
    const profilePath = await realpath(profile),
      outputPath = await realpath(output);
    assert.equal(path.dirname(profilePath), outputPath, 'Owned profile escaped output');
    assert.match(basename(profilePath), /^chrome-[a-zA-Z0-9]+$/);
    await rm(profilePath, {recursive: true, force: true, maxRetries: 10, retryDelay: 100});
    report.owned_profile_removed = true;
  }
  report.owned_browser_closed = chrome
    ? chrome.exitCode !== null || chrome.signalCode !== null
    : true;
  report.owned_server_closed = !server?.listening;
  await durable(
    path.join(output, 'result.json'),
    Buffer.from(JSON.stringify(report, null, 2) + '\n')
  );
}
