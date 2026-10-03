#!/usr/bin/env python3
"""Exercise shipped hardening as an unprivileged transient unit, on isolated paths."""
from apt_e2e import Harness, PROJECT, DAEMON, run
import os
from pathlib import Path
import pwd
import subprocess
import tempfile
import time
import uuid

if os.geteuid() != 0 or Path('/proc/1/comm').read_text().strip() != 'systemd':
    raise SystemExit('This explicit test requires root on a systemd development host')
parent = PROJECT / '.build/systemd'
parent.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='instance-', dir=parent) as temporary:
    root = Path(temporary)
    h = Harness(root)
    unit = 'xxc-aptd-test-' + uuid.uuid4().hex
    account = pwd.getpwnam('nobody')
    subprocess.run(['gpgconf', '--homedir', str(h.keys), '--kill', 'gpg-agent'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for path in [root, *root.rglob('*')]:
        os.chown(path, account.pw_uid, account.pw_gid)
    # A real systemd credential mount exercises root-owned ACL delivery (0440
    # on newer systemd), without exposing a synthetic token to the service group.
    credential=root/'credential-source'
    credential.write_text('synthetic_systemd_credential_123456')
    os.chown(credential,0,0)
    credential.chmod(0o600)
    text=h.config.read_text().replace('[xxc_trust]\nenabled = false','[xxc_trust]\nenabled = true').replace('https://ca.example.invalid/api/v1','https://127.0.0.1:1/api/v1')
    h.config.write_text(text)
    properties = []
    for line in (PROJECT / 'systemd/xxc-aptd.service').read_text().splitlines():
        if line.startswith(('NoNewPrivileges=', 'PrivateTmp=', 'ProtectHome=', 'ProtectSystem=', 'ProtectKernel', 'ProtectControlGroups=', 'RestrictSUIDSGID=', 'LockPersonality=', 'RestrictAddressFamilies=', 'CapabilityBoundingSet=', 'AmbientCapabilities=', 'KillMode=', 'UMask=')):
            properties.extend(['--property', line])
    properties += ['--property', f'User={account.pw_uid}', '--property', f'Group={account.pw_gid}', '--property', f'ReadWritePaths={root}', '--property', f'LoadCredential=xxc-trust-token:{credential}']
    try:
        run(['systemd-run', '--quiet', '--collect', '--unit', unit, *properties, DAEMON, '--config', h.config, 'serve'])
        for _ in range(100):
            try:
                if h.api('GET', 'status')[0] == 200:
                    break
            except (OSError, Exception):
                pass
            time.sleep(.1)
        else:
            raise AssertionError('Hardened unprivileged unit did not become ready')
        assert h.api('GET','trust/status')[0] == 502, 'Unavailable CA must not prevent startup with a valid systemd credential'
        fixture = h.fixture()
        status, package = h.api('POST', 'uploads', fixture.read_bytes(), raw=True)
        assert status == 200
        h.api('POST', f"uploads/{package['id']}/stage", {})
        h.job('repository/publish')
        h.job('repository/verify')
        assert h.fetch('/repo/dists/zerotrust/InRelease')[0] == 200
        print('PASS: shipped hardening permits upload, metadata generation, GPG signing and HTTP and credential loading as an unprivileged systemd unit')
    finally:
        subprocess.run(['systemctl', 'stop', unit], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        h.log.close()
