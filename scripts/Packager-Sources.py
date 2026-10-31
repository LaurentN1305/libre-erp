"""Archive des sources uniquement : aucune donnée AppData ni clé de build."""
from pathlib import Path
import os
import zipfile
import argparse
import re

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--module', default='M4', choices=['P0', 'M1', 'M2', 'M4', 'M3', 'M5', 'M6', 'M7', 'M8'])
parser.add_argument('--revision')
args=parser.parse_args()
module = args.module
if args.revision and not re.fullmatch(r'\d+\.\d+\.\d+',args.revision): parser.error('Revision attendue : x.y.z')
suffix=f'-{args.revision}' if args.revision else ''
destination = root / f'Livraison-{module}{suffix}'
destination.mkdir(exist_ok=True)
prefix='LibreERP' if module in ('M5','M6','M7','M8') else 'SecuriteERP'
archive = destination / f'{prefix}-{module}-sources.zip'
excluded = {'Archives-des-versions', '.tools', 'node_modules', 'target', 'dist', '.git', '.validation-deps', 'Livraison-P0', 'Livraison-M1', '__pycache__'}
with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as output:
    for folder, directories, files in os.walk(root):
        directories[:] = sorted(d for d in directories if d not in excluded and not d.startswith('Livraison-'))
        for filename in sorted(files):
            path = Path(folder) / filename
            if path.is_symlink():
                continue
            if filename.startswith('.env') or path.suffix.lower() in {'.pem', '.pfx', '.p12', '.key', '.erp-cles', '.dpapi', '.exe', '.msi', '.log', '.zip'}:
                continue
            if filename == 'tauri.release.json':
                continue
            output.write(path, path.relative_to(root).as_posix())
print(archive)
