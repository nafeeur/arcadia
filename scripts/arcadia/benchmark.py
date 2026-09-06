#!/usr/bin/env python3
"""Read-only search load probe. Reports measurements, never invents scale claims.
Input JSON: [{"q":"price", "as_of":null, "expected_document_ids":["..."]}]
ARCADIA_TOKEN must be a session JWT (the REST API does not accept MCP-only tokens).
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import statistics
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--url',default='http://localhost:1516')
    p.add_argument('--kb',required=True)
    p.add_argument('--queries',required=True,type=Path)
    p.add_argument('--repeats',type=int,default=5)
    p.add_argument('--concurrency',type=int,default=1)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args()
    token=os.environ.get('ARCADIA_TOKEN')
    if not token:p.error('Set ARCADIA_TOKEN')
    if not 1<=a.concurrency<=32 or not 1<=a.repeats<=100:p.error('Concurrency: 1–32; repeats: 1–100')
    url = urlsplit(a.url)
    if url.username or url.password or url.fragment or not (url.scheme == 'https' or (url.scheme == 'http' and url.hostname in {'localhost','127.0.0.1','::1'})):
        p.error('Use HTTPS, or HTTP on localhost, without URL credentials')
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            return None
    opener = urllib.request.build_opener(NoRedirect())
    queries=json.loads(a.queries.read_text())
    if not isinstance(queries,list) or not queries or len(queries)>10000:p.error('Provide 1–10,000 queries')
    def sample(q):
        start=time.perf_counter()
        req=urllib.request.Request(f'{a.url.rstrip("/")}/api/v1/kbs/{a.kb}/search',data=json.dumps({'q':q['q'],'as_of':q.get('as_of'),'top_k':20}).encode(),headers={'Authorization':f'Bearer {token}','Content-Type':'application/json'})
        try:
            with opener.open(req,timeout=90) as response:result=json.load(response)
            found={r['document_id'] for r in result['results']}
            expected=set(q.get('expected_document_ids',[]))
            return {'ok':True,'ms':(time.perf_counter()-start)*1000,'recall':len(found&expected)/len(expected) if expected else None}
        except (OSError,ValueError,KeyError,TypeError):
            return {'ok':False,'ms':(time.perf_counter()-start)*1000,'recall':None}
    started=time.perf_counter()
    with ThreadPoolExecutor(max_workers=a.concurrency) as pool:rows=list(pool.map(sample,queries*a.repeats))
    elapsed=time.perf_counter()-started
    times=sorted(r['ms'] for r in rows if r['ok'])
    recalls=[r['recall'] for r in rows if r['recall'] is not None]
    report={'requests':len(rows),'successes':len(times),'failures':len(rows)-len(times),'concurrency':a.concurrency,'elapsed_seconds':elapsed,'requests_per_second':len(rows)/elapsed,'p50_ms':statistics.median(times) if times else None,'p95_ms':times[min(len(times)-1,int(len(times)*.95))] if times else None,'mean_document_recall_at_20':statistics.mean(recalls) if recalls else None,'samples':rows,'note':'Dataset size and model configuration must be recorded separately. This measures retrieval, not answer accuracy.'}
    a.output.write_text(json.dumps(report,indent=2))
    print(json.dumps({k:v for k,v in report.items() if k!='samples'},indent=2))
    if report['failures']:raise SystemExit(1)
if __name__=='__main__':main()
