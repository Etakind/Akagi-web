#!/usr/bin/env python3
"""Read only public Cargo lock metadata; query OSV; never inspect local secrets."""
import concurrent.futures, json, pathlib, subprocess, tomllib
ROOT = pathlib.Path(__file__).resolve().parents[1]
def request(url, body=None):
    args=['curl','--fail','--silent','--show-error','--proto','=https','--max-time','60',url]
    if body is not None: args += ['-H','Content-Type: application/json','--data-binary','@-']
    r=subprocess.run(args,input=json.dumps(body) if body is not None else None,capture_output=True,text=True,check=True)
    return json.loads(r.stdout)
packages={}
for lock in ['Cargo.lock','native_bot/Cargo.lock']:
    for p in tomllib.loads((ROOT/lock).read_text())['package']:
        if p.get('source','').startswith('registry+'):
            packages.setdefault((p['name'],p['version']),[]).append(lock)
items=list(packages)
def batch(start):
    subset=items[start:start+100]
    answer=request('https://api.osv.dev/v1/querybatch',{'queries':[{'package':{'name':n,'ecosystem':'crates.io'},'version':v} for n,v in subset]})
    return [(p,v['id']) for p,r in zip(subset,answer['results']) for v in r.get('vulns',[])]
with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    findings=[x for b in pool.map(batch,range(0,len(items),100)) for x in b]
ids=sorted({v for _,v in findings})
with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    details=dict(zip(ids,pool.map(lambda v:request('https://api.osv.dev/v1/vulns/'+v),ids)))
report=[{'name':n,'version':v,'locks':packages[(n,v)],'id':id,'summary':details[id].get('summary'),
    'details':details[id].get('details'),'references':details[id].get('references'),
    'affected':details[id].get('affected'),'withdrawn':details[id].get('withdrawn')} for (n,v),id in findings]
(ROOT/'dependency-audit.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print('CRATE_VERSIONS_CHECKED',len(items))
for r in report:
    print(r['name'],r['version'],r['id'],r['summary'],','.join(r['locks']))
