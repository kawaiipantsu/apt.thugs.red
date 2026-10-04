#!/usr/bin/env python3
"""Scoped project tokens, independent suites, real APT acquisition and traffic privacy."""
from apt_e2e import Harness, apt_acquire, run, CLI
from pathlib import Path
import http.client
import json
import sqlite3
import tempfile
import time
import shutil
import os
import re
import subprocess


def exercise(h):
    h.config.write_text(h.config.read_text().replace('suites = []', 'suites = ["zerotrust", "nightly", "production"]')
                        .replace('trusted_proxies = []', 'trusted_proxies = ["127.0.0.1/32"]'))
    h.start()
    def token(scopes, suites):
        status, result = h.api('POST', 'tokens', {'name':'disposable project', 'scopes':scopes, 'suites':suites, 'days':1})
        assert status == 200
        return result
    credential=token(['read','upload','stage','publish'],['nightly'])
    def request(method,path,data=None,secret=None,raw=False):
        c=http.client.HTTPConnection('127.0.0.1',h.admin_port,timeout=30)
        headers={'Authorization':'Bearer '+(secret or credential['secret']),'Content-Type':'application/octet-stream' if raw else 'application/json'}
        c.request(method,'/admin/api/v1/'+path,body=data if raw else (json.dumps(data).encode() if data is not None else None),headers=headers)
        r=c.getresponse(); body=r.read(); result=r.status,json.loads(body);c.close();return result
    def finish(result):
        assert result[0]==202,result[0]
        for _ in range(400):
            code,job=request('GET','jobs/'+result[1]['job_id']+'?suite=nightly')
            assert code==200
            if job['state']!='running': assert job['state']=='succeeded';return
            time.sleep(.05)
        raise AssertionError('job deadline')
    fixture=h.fixture()
    code, package=request('POST','uploads?suite=nightly',fixture.read_bytes(),raw=True)
    assert code==200,code
    assert request('POST','uploads?suite=production',fixture.read_bytes(),raw=True)[0]==403
    assert request('GET','packages')[0]==403  # default suite is not authorized
    assert request('GET','config?suite=nightly')[0]==403
    assert request('GET','tokens?suite=nightly')[0]==403
    assert request('GET','packages?suite=nightly&suite=production')[0]==400
    assert request('GET','packages?suite=../nightly')[0]==403
    assert h.api('GET','packages?suite=unknown')[0]==400
    # A valid bearer cannot authorize UI and cannot mix with ambient cookies.
    conn=http.client.HTTPConnection('127.0.0.1',h.admin_port)
    conn.request('GET','/admin/',headers={'Authorization':'Bearer '+credential['secret']})
    response=conn.getresponse();assert response.status==403;response.read();conn.close()
    conn=http.client.HTTPConnection('127.0.0.1',h.admin_port)
    conn.request('GET','/admin/api/v1/packages?suite=nightly',headers={'Authorization':'Bearer '+credential['secret'],'Cookie':'fixture=ambient'})
    response=conn.getresponse();assert response.status==401;response.read();conn.close()
    assert request('POST','repository/rollback?suite=nightly',{})[0]==403
    assert request('POST','uploads/'+package['id']+'/stage?suite=nightly',{})[0]==200
    # Stage another package in production. Publishing nightly must not consume it.
    other=h.fixture(version='2.0')
    code, prod=h.api('POST','uploads?suite=production',other.read_bytes(),raw=True);assert code==200
    assert h.api('POST','uploads/'+prod['id']+'/stage?suite=production',{})[0]==200
    assert request('GET','packages/'+prod['id']+'?suite=nightly')[0]==404
    review=request('GET','repository/diff?suite=nightly')[1]
    assert review['suite']=='nightly' and len(review['added'])==1
    finish(request('POST','repository/publish?suite=nightly',{'review_token':review['token']}))
    old=h.api('GET','status')[1]['generation']
    assert b'Version: 1.0' in h.fetch('/repo/dists/nightly/main/binary-amd64/Packages')[2]
    assert h.fetch('/repo/dists/production/main/binary-amd64/Packages')[2]==b''
    assert h.fetch('/repo/dists/zerotrust/main/binary-amd64/Packages')[2]==b''
    assert len(h.api('GET','repository/diff?suite=production')[1]['added'])==1
    assert request('POST','repository/publish?suite=nightly',{'review_token':review['token']})[0]==409
    # Scope checks happen before accepting upload bytes or submitting jobs.
    read=token(['read'],['nightly'])
    assert request('POST','uploads?suite=nightly',fixture.read_bytes(),secret=read['secret'],raw=True)[0]==403
    assert request('POST','repository/publish?suite=nightly',{},secret=read['secret'])[0]==403
    assert request('GET','jobs?suite=nightly',secret=read['secret'])[1]['jobs']==[]
    prodreview=h.api('GET','repository/diff?suite=production')[1]
    h.job('repository/publish?suite=production',{'review_token':prodreview['token']})
    assert b'Version: 1.0' in h.fetch('/repo/dists/nightly/main/binary-amd64/Packages')[2]
    assert b'Version: 2.0' in h.fetch('/repo/dists/production/main/binary-amd64/Packages')[2]
    apt_acquire(h,package,suite='nightly')
    shutil.rmtree(h.root/'apt')
    apt_acquire(h,prod,suite='production')
    for suite in ['nightly','production']:
        assert h.fetch('/releases/'+suite)[0]==200
        assert ('Suites: '+suite).encode() in h.fetch('/repo/thugsred.sources?suite='+suite)[2]
    h.job('repository/rollback',{'generation':old})
    assert h.fetch('/repo/dists/production/main/binary-amd64/Packages')[2]==b''
    assert b'Version: 1.0' in h.fetch('/repo/dists/nightly/main/binary-amd64/Packages')[2]
    # Shared bytes acquire a new explicit membership; no cross-suite read bypass.
    assert h.api('POST','uploads?suite=production',fixture.read_bytes(),raw=True)[1]['id']==package['id']
    assert h.api('GET','packages/'+package['id']+'?suite=production')[0]==200
    # Execute the exact downloadable documentation example against a disposable
    # signed repository. Do not maintain a separate hand-written client example.
    status,_,guide=h.fetch('/api/guide.md')
    assert status==200
    script=re.findall(r'```sh\n(.*?)\n```',guide.decode(),re.S)
    assert len(script)==1
    secret_file=h.root/'ci-token';secret_file.write_text(credential['secret']);secret_file.chmod(0o600)
    documented_fixture=h.fixture(version='3.0')
    env={**os.environ,'XXC_API_BASE':h.admin_origin+'/admin/api/v1','XXC_SUITE':'nightly','XXC_TOKEN_FILE':str(secret_file),'XXC_DEB':str(documented_fixture)}
    script_result=subprocess.run(['sh','-c',script[0]],env=env,capture_output=True,timeout=60)
    assert script_result.returncode==0, 'Published documentation example failed'
    assert credential['secret'].encode() not in script_result.stdout+script_result.stderr
    # The example's policy guard must reject a different staged upload.
    pending=h.fixture(version='4.0',name='xxc-policy-fixture')
    code,pending_package=request('POST','uploads?suite=nightly',pending.read_bytes(),raw=True);assert code==200
    assert request('POST','uploads/'+pending_package['id']+'/stage?suite=nightly',{})[0]==200
    before_generation=h.api('GET','status')[1]['generation']
    env['XXC_DEB']=str(h.fixture(version='5.0'))
    script_result=subprocess.run(['sh','-c',script[0]],env=env,capture_output=True,timeout=60)
    assert script_result.returncode!=0
    assert h.api('GET','status')[1]['generation']==before_generation
    secret_file.unlink()
    cli=json.loads(run([CLI,'--socket',h.root/'run/admin.sock','--suite','nightly','--json','package','list']))
    assert any(p['id']==package['id'] for p in cli['packages'])
    time.sleep(1.2)
    before=h.api('GET','analytics?days=7')[1]['statistics']
    path='/repo/'+package['filename']
    client={'X-Forwarded-For':'192.0.2.19'}
    _,headers,_=h.fetch(path,headers=client)
    assert h.fetch(path,method='HEAD',headers=client)[0]==200
    assert h.fetch(path,headers={**client,'Range':'bytes=0-9'})[0]==206
    assert h.fetch(path,headers={**client,'If-None-Match':headers['etag']})[0]==304
    assert h.fetch('/search?q=private-search-fixture',headers=client)[0]==200
    time.sleep(1.2)
    after=h.api('GET','analytics?days=7')[1]['statistics']
    for key,delta in [('requests',4),('downloads',1),('ranges',1),('not_modified',1)]:
        assert after['totals'][key]-before['totals'][key]==delta,(key,before['totals'],after['totals'])
    assert after['clients']==before['clients']+1
    assert h.fetch('/admin/api/v1/analytics')[0]==404
    assert h.fetch('/api/v1/tokens')[0]==404
    assert request('GET','analytics?suite=nightly')[0]==403
    db=sqlite3.connect(h.root/'state/state.db')
    db.execute('UPDATE api_tokens SET expires=0 WHERE id=?',(read['token']['id'],));db.commit()
    assert request('GET','packages?suite=nightly',secret=read['secret'])[0]==401
    h.stop();h.start()
    assert request('GET','packages?suite=nightly')[0]==200
    assert h.api('GET','analytics?days=7')[1]['statistics']['totals']==after['totals']
    assert h.api('POST','tokens/'+credential['token']['id']+'/revoke',{})[0]==200
    assert request('GET','packages?suite=nightly')[0]==401
    # Recovered legacy manifests preserve default suite membership.
    assert h.api('GET','tokens')[1]['tokens'][0].get('digest') is None
    serialized=json.dumps(h.api('GET','audit')[1])
    assert credential['secret'] not in serialized
    for path in (h.root/'state').glob('state.db*'):
        blob=path.read_bytes()
        assert credential['secret'].encode() not in blob
        assert b'192.0.2.19' not in blob
        assert b'private-search-fixture' not in blob
    db.close()

if __name__=='__main__':
    with tempfile.TemporaryDirectory(prefix='xxc-automation-') as root:
        h=Harness(Path(root))
        try:exercise(h)
        finally:h.stop();h.log.close()
    print('automation: scoped tokens, isolated suites, real APT, rollback and analytics passed')
