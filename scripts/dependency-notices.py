#!/usr/bin/env python3
"""Collect locked dependency metadata/notices for distribution review.
Includes dev and cross-platform packages; review the inventory before release.
"""
import json
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
out = root / 'dist' / 'production-review' / 'dependencies'
if out.exists():
    raise SystemExit('Dependency review output already exists; preserve it or choose a fresh output.')
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--offline', '--format-version', '1'], cwd=root))
resolved = {node['id'] for node in metadata['resolve']['nodes']}
items = []
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    if package['id'] not in resolved or package['name'] == 'desklink':
        continue
    folder = Path(package['manifest_path']).parent
    files = [p for p in folder.iterdir() if p.is_file() and p.name.lower().startswith(('license', 'licence', 'notice', 'copyright', 'copying'))]
    if package.get('license_file'):
        candidate = folder / package['license_file']
        if candidate.is_file() and candidate not in files:
            files.append(candidate)
    dest = out / 'notices' / (package['name'] + '-' + package['version'])
    dest.mkdir(parents=True, exist_ok=True)
    for source in files:
        shutil.copyfile(source, dest / source.name)
    items.append({'name': package['name'], 'version': package['version'], 'license': package['license'], 'repository': package['repository'], 'notice_files': sorted(p.name for p in files)})
(out / 'inventory.json').write_text(json.dumps({'scope': 'locked graph, including development and platform-specific dependencies', 'packages': items}, indent=2) + '\n')
(out / 'README.txt').write_text('Dependency license review material for DeskLink. This inventory includes development and platform-specific dependencies. Resolve license alternatives, missing text and native bundled components before production distribution. It is not an assertion that every listed dependency is shipped.\n')
print(f'Collected {len(items)} package records; {sum(not p["notice_files"] for p in items)} require source notice review.')
