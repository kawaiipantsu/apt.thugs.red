#!/usr/bin/env python3
"""APT acquisition through a TLS CA holding all signing secrets outside the daemon."""
from apt_e2e import Harness, CLI, run, apt_acquire
from admin_e2e import Browser, PASSWORD
from trust_e2e import tls_server, TOKEN, OMITTED
from http.server import BaseHTTPRequestHandler
from pathlib import Path
from urllib.parse import urlsplit, parse_qs
import base64
import copy
import hashlib
import json
import os
import shutil
import ssl
import tempfile
import threading
import time
import uuid

ID='1'*32
class SigningCA(BaseHTTPRequestHandler):
    mode='ok'
    keys=[]
    home=None
    requests=[]
    signing=threading.Event()
    proceed=threading.Event()

    def log_message(self,*args):pass

    def reply(self,data,status=200):
        self.send_response(status)
        body=data if isinstance(data,bytes) else json.dumps(data).encode()
        self.send_header('Content-Type','application/json')
        self.send_header('Content-Length',str(len(body)))
        self.end_headers();self.wfile.write(body)

    def do_GET(self):
        assert self.headers['Authorization']=='Bearer '+TOKEN
        parsed=urlsplit(self.path);SigningCA.requests.append(('GET',parsed.path))
        if parsed.path in ['/api/v1/authorities','/api/v1/templates']:
            self.reply({'items':[]});return
        if parsed.path=='/api/v1/openpgp/keys':
            q=parse_qs(parsed.query);page=int(q.get('page',[1])[0])
            self.reply({'items':SigningCA.keys[page-1:page],'page':page,'pages':len(SigningCA.keys),'total':len(SigningCA.keys)});return
        parts=parsed.path.split('/')
        if len(parts)<6 or parts[4]!='keys':self.reply({},404);return
        key=next((k for k in SigningCA.keys if k['id']==parts[5]),None)
        if not key:self.reply({},404);return
        if parsed.path.endswith('/download'):
            assert parse_qs(parsed.query)=={'format':['binary']}
            mode='--export-secret-keys' if SigningCA.mode=='secret-export' else '--export'
            fingerprint=SigningCA.keys[1]['fingerprint'] if SigningCA.mode=='wrong-public' else key['fingerprint']
            self.reply(run(['gpg','--batch','--homedir',SigningCA.home,mode,fingerprint]));return
        key=copy.deepcopy(key)
        if SigningCA.mode in ['revoked','expired']:key['status']=SigningCA.mode
        if SigningCA.mode=='public-only':key['has_private_key']=False
        if SigningCA.mode=='no-signing':key['capabilities']='e'
        if SigningCA.mode=='wrong-identity':key['fingerprint']='A'*40
        self.reply(key)

    def do_POST(self):
        assert self.headers['Authorization']=='Bearer '+TOKEN
        SigningCA.requests.append(('POST',self.path))
        body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path=='/api/v1/openpgp/keys':
            assert set(body)=={'name','email','algorithm','days'}
            identity=f"{body['name']} <{body['email']}>"
            run(['gpg','--batch','--homedir',SigningCA.home,'--pinentry-mode','loopback','--passphrase','','--quick-generate-key',identity,'ed25519','sign','1d'])
            listing=run(['gpg','--homedir',SigningCA.home,'--with-colons','--list-keys',identity]).decode()
            fp=next(line.split(':')[9] for line in listing.splitlines() if line.startswith('fpr:'))
            key=metadata(uuid.uuid4().hex,fp,body['name']);SigningCA.keys.append(key);self.reply(key,201);return
        assert self.path=='/api/v1/debian/sign' and set(body)=={'key_id','kind','data_base64'}
        assert body['key_id']==ID and body['kind']=='release'
        if SigningCA.mode=='denied':self.reply({'secret':OMITTED},403);return
        if SigningCA.mode=='blocked':
            SigningCA.signing.set();SigningCA.proceed.wait(15)
        data=base64.b64decode(body['data_base64'],validate=True)
        if SigningCA.mode=='wrong-payload':data+=b'X-Fixture: changed\n'
        with tempfile.TemporaryDirectory(dir=SigningCA.home.parent) as d:
            root=Path(d);release=root/'Release';release.write_bytes(data)
            fp=SigningCA.keys[1]['fingerprint'] if SigningCA.mode=='wrong-signature' else SigningCA.keys[0]['fingerprint']
            for mode,name in [('--clearsign','InRelease'),('--detach-sign','Release.gpg')]:
                run(['gpg','--batch','--yes','--homedir',SigningCA.home,'--local-user',fp,'--digest-algo','SHA256','--output',root/name,mode,release])
            artifacts=[{'filename':name,'content_type':'application/pgp-signature','data_base64':base64.b64encode((root/name).read_bytes()).decode()} for name in ['InRelease','Release.gpg']]
        if SigningCA.mode=='path':artifacts[0]['filename']='../../escape'
        if SigningCA.mode=='duplicate':artifacts[0]['filename']='Release.gpg'
        if SigningCA.mode=='missing':artifacts.pop()
        if SigningCA.mode=='bad-base64':artifacts[0]['data_base64']='not base64!'
        if SigningCA.mode=='corrupt-signature':artifacts[1]['data_base64']=base64.b64encode(b'corrupt').decode()
        self.reply({'fingerprint':'A'*40 if SigningCA.mode=='wrong-fingerprint' else SigningCA.keys[0]['fingerprint'],'artifacts':artifacts})


def metadata(key_id,fingerprint,label='Fixture archive'):
    return {'id':key_id,'fingerprint':fingerprint,'label':label,'status':'active','algorithm':'EdDSA','bits':255,'capabilities':'sc','not_after':None,'has_private_key':True,'private_key':OMITTED,'emails':[OMITTED],'user_ids':[OMITTED]}


def configure(h):
    run(['gpgconf','--homedir',h.keys,'--kill','gpg-agent'])
    home=h.root/'remote-keys'
    h.keys.rename(home);h.keys.mkdir(mode=0o700)
    SigningCA.home=home;SigningCA.keys=[metadata(ID,h.fingerprint)];SigningCA.mode='ok'
    ca,certificate=tls_server(h.root,SigningCA)
    credentials=h.root/'credentials';credentials.mkdir(mode=0o700)
    token=credentials/'xxc-trust-token';token.write_text(TOKEN);token.chmod(0o600)
    text=h.config.read_text().replace('backend = "gpg"','backend = "xxc-trust"').replace('remote_key_id = ""',f'remote_key_id = "{ID}"').replace('[xxc_trust]\nenabled = false','[xxc_trust]\nenabled = true').replace('https://ca.example.invalid/api/v1',f'https://127.0.0.1:{ca.server_port}/api/v1')
    text+=f'\nca_certificate = "{certificate}"\n'
    h.config.write_text(text)
    return ca,{**os.environ,'CREDENTIALS_DIRECTORY':str(credentials)}


def exercise(h):
    ca,env=configure(h)
    try:
        h.start(env)
        socket=h.root/'run/admin.sock'
        for role in ['administrator','operator','viewer']:
            run([CLI,'--socket',socket,'user','add',role,'--role',role,'--password-stdin'],input=(PASSWORD+'\n').encode())
        for role in ['viewer','operator']:
            b=Browser(h);assert b.login(role)[0]==200
            assert b.request('GET','/api/v1/keys')[0]==403
            assert b.request('GET','/admin/keys')[0]==403
            assert b.request('POST','/api/v1/keys',{'name':'invalid'})[0]==403
        admin=Browser(h);assert admin.login('administrator')[0]==200
        assert admin.request('POST','/api/v1/keys',{},headers={'X-CSRF-Token':'wrong'})[0]==403
        created=json.loads(run([CLI,'--socket',socket,'--json','key','generate','--name','Fixture next archive','--email','archive@example.invalid']))
        assert created['id']!=ID and created['fingerprint']!=h.fingerprint
        assert 'emails' not in created and 'user_ids' not in created and OMITTED not in json.dumps(created)
        keys=json.loads(run([CLI,'--socket',socket,'--json','key','list','--page','2']))
        assert keys['page']==2 and keys['items'][0]['id']==created['id']
        assert admin.request('GET','/admin/keys')[0]==200
        assert admin.request('GET','/api/v1/keys/'+ID+'/public?format=secret')[0]==400
        assert admin.request('GET','/api/v1/keys/'+ID+'/download')[0]==404
        verified=json.loads(run([CLI,'--socket',socket,'--json','key','verify',ID]))
        assert verified['public_key_valid'] and verified['fingerprint']==h.fingerprint
        export=h.root/'export.asc';run([CLI,'--socket',socket,'key','export',ID,'--armor','--output',export])
        assert export.read_bytes().startswith(b'-----BEGIN PGP PUBLIC KEY BLOCK-----')
        assert not list(h.keys.iterdir()),'Remote secret or public keys persisted in daemon key directory'
        fixture=h.fixture();status,pkg=h.api('POST','uploads',fixture.read_bytes(),raw=True);assert status==200
        h.api('POST','uploads/'+pkg['id']+'/stage',{})
        h.job('repository/publish')
        initial=h.api('GET','status')[1]['generation']
        apt_acquire(h,pkg)
        old_index=h.fetch('/repo/dists/zerotrust/main/binary-amd64/Packages.xz')[2]
        old_hash=hashlib.sha256(old_index).hexdigest()
        h.job('repository/publish')
        latest=h.api('GET','status')[1]['generation'];assert latest!=initial
        assert h.fetch('/repo/dists/zerotrust/main/binary-amd64/by-hash/SHA256/'+old_hash)[2]==old_index
        for mode in ['wrong-identity','public-only','no-signing','revoked','expired','wrong-public','secret-export','denied','path','duplicate','missing','bad-base64','wrong-fingerprint','wrong-signature','wrong-payload','corrupt-signature']:
            SigningCA.mode=mode
            h.job('repository/publish',success=False)
            assert h.api('GET','status')[1]['generation']==latest,mode
            assert h.fetch('/repo/'+pkg['filename'])[2]==fixture.read_bytes()
            assert not (h.root/'escape').exists()
            assert not list(h.keys.iterdir())
        SigningCA.mode='blocked'
        status,job=h.api('POST','repository/publish',{'review_token':h.api('GET','repository/diff')[1]['token']});assert status==202
        assert SigningCA.signing.wait(5)
        assert h.fetch('/repo/'+pkg['filename'])[0]==200
        assert h.api('POST','repository/publish',{'review_token':'stale'})[0]==409
        SigningCA.proceed.set()
        for _ in range(100):
            state=h.api('GET','jobs/'+job['job_id'])[1]['state']
            if state!='running':break
            time.sleep(.05)
        assert state=='succeeded'
        SigningCA.mode='ok'
        h.stop();ca.shutdown();ca.server_close()
        # Startup, verification, key distribution and rollback require no CA.
        h.start(env)
        h.job('repository/verify')
        h.job('repository/rollback',{'generation':initial})
        assert h.api('GET','status')[1]['generation']==initial
        assert h.fetch('/repo/thugsred.gpg.key')[0]==200
        h.job('repository/publish',success=False)
        assert h.api('GET','status')[1]['generation']==initial
        assert not list(h.keys.iterdir())
        for path in h.root.rglob('*.log'):
            assert TOKEN.encode() not in path.read_bytes() and OMITTED.encode() not in path.read_bytes()
    finally:
        SigningCA.proceed.set();h.stop();ca.shutdown();ca.server_close()
        run(['gpgconf','--homedir',SigningCA.home,'--kill','gpg-agent'])


if __name__=='__main__':
    with tempfile.TemporaryDirectory(prefix='xxc-remote-signing-') as root:
        h=Harness(Path(root))
        try:exercise(h)
        except Exception:
            target=Path('/tmp/xxc-remote-test-failure.log')
            shutil.copyfile(h.root/'process.log',target);target.chmod(0o600)
            raise
        finally:h.stop()
    print('PASS: remote-held OpenPGP keys, real APT update/download, pinned verification, failure atomicity, offline rollback and key CLI/API')
