#!/usr/bin/env python3
"""Package a reviewed Windows payload; this helper never builds or signs app code."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile


def nsis_string(value):
    if any(c in value for c in '\r\n\0'):
        raise ValueError('Unsupported newline/NUL in a package path')
    return value.replace('$', '$$').replace('"', '$\\"')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--payload', type=Path, required=True)
    parser.add_argument('--arch', choices=['x64', 'arm64'], required=True)
    parser.add_argument('--output', type=Path, required=True,
                        help='New output directory; existing paths are refused')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    package = (root / 'Cargo.toml').read_text().split('[package]', 1)[1].split('\n[', 1)[0]
    version = re.search(r'^version\s*=\s*"([0-9.]+)"', package, re.MULTILINE).group(1)
    assert len(version.split('.')) == 3 and all(v.isdigit() for v in version.split('.'))
    payload = args.payload.resolve(strict=True)
    files = sorted(p for p in payload.rglob('*') if p.is_file())
    assert not any(p.is_symlink() for p in payload.rglob('*')), 'Symlinks are not payload files'
    assert {p.name for p in payload.iterdir()} == {
        'desklink.exe', 'README.txt', 'LICENSE.txt', 'THIRD-PARTY-NOTICES'}
    assert (payload / 'THIRD-PARTY-NOTICES/inventory.json').is_file()
    assert all(p.suffix.lower() not in ['.pfx', '.p12', '.key', '.sqlite3', '.xml', '.log'] for p in files)
    exe = (payload / 'desklink.exe').read_bytes()
    assert exe[:2] == b'MZ'
    pe = struct.unpack_from('<I', exe, 0x3c)[0]
    assert exe[pe:pe+4] == b'PE\0\0'
    assert struct.unpack_from('<H', exe, pe+4)[0] == {'x64': 0x8664, 'arm64': 0xaa64}[args.arch]
    assert version.encode('utf-16le') in exe, 'Executable version differs from source'
    assert struct.unpack_from('<H', exe, pe+24)[0] == 0x20b
    assert struct.unpack_from('<II', exe, pe+24+112+32) == (0, 0), 'This builder is for unsigned previews'
    compiler = shutil.which('makensis')
    if not compiler:
        raise SystemExit('Install NSIS 3.13 (makensis) first.')
    compiler_version = subprocess.check_output([compiler, '-VERSION'], text=True).strip()
    if compiler_version != 'v3.13':
        raise SystemExit('This reviewed preview packager requires NSIS 3.13 and its matching notice.')
    args.output.mkdir(parents=True, exist_ok=False)
    output = args.output.resolve()
    installer = output / f'DeskLink-{version}-windows-{args.arch}-setup.exe'
    with tempfile.TemporaryDirectory(prefix='.installer-', dir=output) as tmp:
        temp = Path(tmp)
        staged = temp / 'payload'
        shutil.copytree(payload, staged)
        shutil.copyfile(root / 'packaging/windows/NSIS-COPYING.txt',
                        staged / 'THIRD-PARTY-NOTICES/NSIS-COPYING.txt')
        (staged / 'README.txt').write_text(
            f'DeskLink {version} — {args.arch} unsigned preview\n\n'
            'Open DeskLink from the Start menu. Each computer receives its ID\n'
            'automatically; every remote session still requires host approval.\n'
            'Screen sharing/control uses outgoing encrypted relay connections.\n'
            'No administrator rights, background service, startup entry or\n'
            'incoming firewall rule is added by this installer.\n\n'
            'Publisher signing is not available. The installer, application and\n'
            'uninstaller are unsigned. Windows may warn or block opening them.\n'
            'Control is limited to the ordinary signed-in desktop, excluding\n'
            'protected/elevated windows and security prompts.\n\n'
            'Close DeskLink before upgrading or removing it. Uninstall through\n'
            'Windows Installed apps or the Start menu. Your private computer ID,\n'
            'settings and unrelated files are retained. Keep the private identity\n'
            'folder to retain your ID when reinstalling.\n\n'
            'Both peers need 0.3.3 for Sharp, Balanced and Original / Retina,\n'
            'Fit / 100% zoom and panning. Higher resolution uses more bandwidth\n'
            'and may lower frame rate. ARM64 is an experimental Windows 11 build;\n'
            'x64 requires 64-bit Windows 10 or later.\n\n'
            'Preview software: no independent security audit or production-scale\n'
            'guarantee. https://desklink.161.97.168.233.nip.io/\n', encoding='utf-8')
        files = sorted(p for p in staged.rglob('*') if p.is_file())
        install_lines, delete_lines = [], []
        for file in files:
            rel = file.relative_to(staged)
            parent = '' if rel.parent == Path('.') else '\\' + str(rel.parent).replace('/', '\\')
            install_lines += [f'SetOutPath "$INSTDIR{nsis_string(parent)}"',
                              f'File "{nsis_string(str(file))}"']
            winrel = nsis_string(str(rel).replace('/', '\\'))
            delete_lines += ['ClearErrors', f'Delete "$INSTDIR\\{winrel}"',
                             '${If} ${Errors}', '  IntOp $9 $9 + 1', '${EndIf}']
        directories = sorted((p for p in staged.rglob('*') if p.is_dir()),
                             key=lambda p: len(p.parts), reverse=True)
        for directory in directories:
            winrel = nsis_string(str(directory.relative_to(staged)).replace('/', '\\'))
            delete_lines.append(f'RMDir "$INSTDIR\\{winrel}"')
        install_include, delete_include = temp / 'install.nsh', temp / 'uninstall.nsh'
        install_include.write_text('\n'.join(install_lines)+'\n', encoding='utf-8')
        delete_include.write_text('\n'.join(delete_lines)+'\n', encoding='utf-8')
        command = [compiler, '-V3', f'-DVERSION={version}', f'-DARCH={args.arch}',
                   f'-DAPP_ICON={root / "assets/icons/desklink.ico"}',
                   f'-DOUTPUT={installer}', f'-DINSTALL_FILES={install_include}',
                   f'-DUNINSTALL_FILES={delete_include}',
                   f'-DINSTALLED_KB={(sum(f.stat().st_size for f in files)+1023)//1024}',
                   str(root / 'packaging/windows/installer.nsi')]
        result = subprocess.run(command, capture_output=True, text=True)
        (output / 'compiler.log').write_text(result.stdout + result.stderr)
        if result.returncode:
            raise SystemExit(result.stdout + result.stderr)
    data = installer.read_bytes()
    receipt = {'version': version, 'architecture': args.arch, 'format': 'EXE installer',
               'archive_name': installer.name, 'bytes': len(data),
               'sha256': hashlib.sha256(data).hexdigest(),
               'executable_sha256': hashlib.sha256(exe).hexdigest(),
               'publisher_signed': False, 'per_user': True,
               'preserves_identity_on_uninstall': True,
               'compiler': compiler_version}
    (output / 'installer-release.json').write_text(json.dumps(receipt, indent=2)+'\n')
    (output / 'SHA256SUMS.txt').write_text(receipt['sha256']+'  '+installer.name+'\n')
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
