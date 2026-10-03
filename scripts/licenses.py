#!/usr/bin/env python3
"""Collect notices for the actual Rust dependency graph of shipped binaries."""
import json
import hashlib
import os
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
os.chdir(root)
target = os.environ.get('CARGO_BUILD_TARGET')
if not target:
    target = next(line.split(': ', 1)[1] for line in subprocess.check_output(['rustc', '-vV'], text=True).splitlines() if line.startswith('host: '))
tree = subprocess.check_output(['cargo', 'tree', '--locked', '-p', 'xxc-aptd', '-p', 'xxc-apt-cli', '--edges', 'normal,build', '--target', target, '--prefix', 'none', '--format', '{p}'], text=True)
used = {(line.split()[0], line.split()[1].removeprefix('v')) for line in tree.splitlines() if line.strip()}
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1', '--filter-platform', target]))
output = root / '.build/dependency-licenses'
if output.exists():
    shutil.rmtree(output)
output.mkdir(parents=True)
inventory = []
for package in metadata['packages']:
    if package['source'] is None or (package['name'], package['version']) not in used:
        continue
    directory = Path(package['manifest_path']).parent
    notices = [p for p in directory.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE', 'COPYING', 'NOTICE', 'UNLICENSE'))]
    if package.get('license_file'):
        notices.append(directory / package['license_file'])
    if not notices:
        supplement = root / 'licenses' / f"{package['name']}-{package['version']}"
        if (supplement / 'provenance.json').is_file():
            provenance = json.loads((supplement / 'provenance.json').read_text())
            vcs = json.loads((directory / '.cargo_vcs_info.json').read_text())
            if any(provenance.get(k) != package[k] for k in ('name', 'version', 'license')) or provenance.get('commit') != vcs['git']['sha1']:
                raise SystemExit('Supplemental notice does not match the locked dependency')
            for filename, digest in provenance['files'].items():
                source = supplement / filename
                if source.parent != supplement or hashlib.sha256(source.read_bytes()).hexdigest() != digest:
                    raise SystemExit('Supplemental notice integrity check failed')
                notices.append(source)
            if notices:
                notices.append(supplement / 'provenance.json')
    if not notices:
        raise SystemExit(f"Dependency is missing license notices: {package['name']}")
    destination = output / f"{package['name']}-{package['version']}"
    destination.mkdir()
    for source in set(notices):
        shutil.copyfile(source, destination / source.name)
    inventory.append({k: package[k] for k in ('name', 'version', 'license', 'repository')})
(output / 'inventory.json').write_text(json.dumps(inventory, indent=2) + '\n')
print(f'Collected license notices for {len(inventory)} compiled dependencies')
