import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class InstallPreservationTests(unittest.TestCase):
    def test_install_and_uninstall_preserve_local_data(self):
        # Test the installation policy with tiny fixture executables/assets.
        with tempfile.TemporaryDirectory(prefix='xxc-install-test-') as temporary:
            base = Path(temporary)
            project = base / 'project'
            script = project / 'scripts/install.py'
            script.parent.mkdir(parents=True)
            script.write_bytes((ROOT / 'scripts/install.py').read_bytes())
            sources = ['target/release/xxc-aptd', 'target/release/xxc-apt-cli',
                       'systemd/xxc-aptd.service', 'systemd/xxc-aptd.sysusers',
                       'systemd/xxc-aptd.tmpfiles', 'debian/xxc-aptd.logrotate',
                       '.build/man/xxc-aptd.8.gz', '.build/man/xxc-apt-cli.1.gz',
                       '.build/man/aptd.conf.5.gz', 'config/aptd.conf.example',
                       '.build/dependency-licenses/inventory.json']
            for source in sources:
                path = project / source
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'distributed fixture')
            destination = base / 'destination'
            preserved = ['etc/xxc/aptd.conf', 'etc/xxc/apt.keys/private-fixture',
                         'var/lib/xxc-aptd/state.db', 'var/lib/xxc-aptd/repository/pool/fixture']
            for relative in preserved:
                path = destination / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'operator-managed sentinel')
            env = dict(os.environ, DESTDIR=str(destination), PREFIX='/usr', SYSCONFDIR='/etc', LOCALSTATEDIR='/var')
            for action in ['install', 'install', 'uninstall']:
                subprocess.run(['python3', str(script), action], env=env, check=True, stdout=subprocess.DEVNULL)
                for relative in preserved:
                    self.assertEqual((destination / relative).read_bytes(), b'operator-managed sentinel')
            self.assertFalse((destination / 'usr/sbin/xxc-aptd').exists())
