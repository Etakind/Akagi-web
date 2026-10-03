// Exercise the real compositor and production CSP in a disposable browser.
// Never attaches to an existing browser/profile or opens a game page.
import { build } from 'vite'
import { mahgenCsp } from './mahgen-csp.mjs'
import { mkdtemp, readFile, rm, stat } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { createServer } from 'node:http'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import assert from 'node:assert/strict'

const root = resolve(import.meta.dirname, '../..')
const tmp = await mkdtemp(join(tmpdir(), 'akagi-tiles-'))
const csp = JSON.parse(await readFile(join(root, 'tauri.conf.json'), 'utf8')).app.security.csp
const candidates = [process.env.AKAGI_TEST_BROWSER,
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome', '/usr/bin/chromium',
].filter(Boolean)
let browser, socket, server
try {
  let executable
  for (const candidate of candidates) {
    if (await stat(candidate).then(s => s.isFile(), () => false)) { executable = candidate; break }
  }
  assert(executable, 'Set AKAGI_TEST_BROWSER to an installed Chromium executable')
  await build({ configFile: false, plugins: [mahgenCsp()], root: join(root, 'frontend'), logLevel: 'error',
    build: { outDir: join(tmp, 'assets'), lib: { entry: join(root, 'frontend/src/lib/tileRenderer.ts'), formats: ['es'], fileName: () => 'tiles.js' } } })
  const tiles = await readFile(join(tmp, 'assets/tiles.js'))
  if (process.platform === 'darwin') {
    const compiler = spawn('/usr/bin/clang', ['-fobjc-arc', '-framework', 'Cocoa', '-framework', 'WebKit',
      join(root, 'frontend/scripts/test-tiles-webkit.m'), '-o', join(tmp, 'webkit-fixture')], { stdio: 'inherit' })
    assert.equal((await once(compiler, 'exit'))[0], 0, 'Native WebKit fixture must compile')
  }
  // Includes red fives, honours, sideways/stacked tiles and river rendering.
  const fixture = `const samples = ['123456789m123p11z', '0m0p0s1234567z', '_555m123p', '123456789s'];
    for (let i=0;i<samples.length;i++) { const e=document.createElement('akagi-tiles');
      if(i===3)e.setAttribute('data-river-mode',''); e.setAttribute('data-seq',samples[i]); document.body.append(e); }
    window.fixtureReady=true;`
  let policy = csp
  server = createServer((req, res) => {
    res.setHeader('Content-Security-Policy', policy)
    if (req.url === '/tiles.js') { res.setHeader('Content-Type','text/javascript'); res.end(tiles) }
    else if (req.url === '/fixture.js') { res.setHeader('Content-Type','text/javascript'); res.end(fixture) }
    else { res.setHeader('Content-Type','text/html'); res.end('<!doctype html><script type="module" src="/tiles.js"></script><script type="module" src="/fixture.js"></script>') }
  }).listen(0, '127.0.0.1')
  await once(server, 'listening')
  browser = spawn(executable, ['--headless=new', '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1',
    `--user-data-dir=${join(tmp, 'profile')}`, '--no-first-run', '--disable-background-networking', 'about:blank'], { stdio: 'ignore' })
  const sleep = ms => new Promise(r => setTimeout(r,ms))
  let port
  for (let i=0;i<200;i++) {
    port = await readFile(join(tmp,'profile/DevToolsActivePort'),'utf8').then(s=>Number(s.split('\n')[0]),()=>null)
    if(port) break
    if(browser.exitCode!==null) throw Error('Disposable browser exited')
    await sleep(100)
  }
  assert(port, 'Disposable browser debugging did not start')
  const pages = await fetch(`http://127.0.0.1:${port}/json/list`).then(r=>r.json())
  socket = new WebSocket(pages.find(p=>p.type==='page').webSocketDebuggerUrl)
  await new Promise((r,j)=>{socket.onopen=r;socket.onerror=j})
  let id=0
  const pending=new Map()
  socket.onmessage=e=>{const m=JSON.parse(e.data);if(m.method==='Runtime.exceptionThrown') console.error(m.params.exceptionDetails.exception?.description?.slice(0,500) ?? 'Script exception');const p=pending.get(m.id);if(p){pending.delete(m.id);m.error?p.reject(Error('CDP command failed')):p.resolve(m.result)}}
  const send=(method,params={})=>new Promise((resolve,reject)=>{const key=++id;pending.set(key,{resolve,reject});socket.send(JSON.stringify({id:key,method,params}))})
  const value=async expression=>(await send('Runtime.evaluate',{expression,returnByValue:true})).result.value
  await send('Page.enable')
  await send('Runtime.enable')
  await send('Log.enable')
  const url=`http://127.0.0.1:${server.address().port}/`
  for (const blocked of [false,true]) {
    policy=blocked?csp.replace("worker-src 'self' blob:","worker-src 'none'"):csp
    await send('Page.navigate',{url:`${url}?blocked=${blocked}`})
    let passed=false
    for(let i=0;i<200;i++) {
      passed=await value(`window.fixtureReady === true && [...document.querySelectorAll('akagi-tiles')].length === 4 && [...document.querySelectorAll('akagi-tiles')].every(e => ${blocked ? "e.hasAttribute('data-render-error') && e.shadowRoot.querySelector('img').alt === '⚠'" : "e.shadowRoot.querySelector('img').naturalWidth > 0 && !e.hasAttribute('data-render-error')"})`)
      if(passed)break
      await sleep(100)
    }
    if(!passed) console.log(await value(`JSON.stringify({ready:window.fixtureReady,tiles:[...document.querySelectorAll('akagi-tiles')].map(e=>({error:e.hasAttribute('data-render-error'),width:e.shadowRoot?.querySelector('img')?.naturalWidth}))})`))
    assert(passed,blocked?'Blocked Worker must show an error placeholder':'All tile samples must render with production CSP')
    if(process.platform==='darwin') {
      const child=spawn(join(tmp,'webkit-fixture'),[url,blocked?'blocked':'allowed'],{stdio:['ignore','pipe','pipe']})
      let output=''
      child.stdout.on('data',b=>{output+=b})
      child.stderr.on('data',b=>{output+=b})
      const kill=setTimeout(()=>child.kill('SIGKILL'),90000)
      const [code]=await once(child,'exit');clearTimeout(kill)
      assert.equal(code,0,output)
      console.log(output.trim())
    }
  }
  console.log('PASS: real tile images under production CSP; blocked Worker shows safe fallback')
} finally {
  socket?.close()
  if(browser && browser.exitCode===null) {browser.kill(); await Promise.race([once(browser,'exit'),new Promise(r=>setTimeout(r,3000))]); if(browser.exitCode===null)browser.kill('SIGKILL')}
  server?.closeAllConnections(); server?.close()
  await rm(tmp,{recursive:true,force:true,maxRetries:8,retryDelay:300})
}
