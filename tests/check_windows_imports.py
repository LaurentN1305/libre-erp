"""Diagnostic des exports Windows nommés dans une sortie dumpbin /imports."""
import ctypes
import re
import sys
from pathlib import Path

loader = ctypes.WinDLL('kernel32', use_last_error=True)
loader.GetProcAddress.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
loader.GetProcAddress.restype = ctypes.c_void_p
dll = None
missing = []
checked = 0
for line in Path(sys.argv[1]).read_text(encoding='utf-8-sig', errors='replace').splitlines():
    match = re.match(r'^\s{4}(\S+\.dll)\s*$', line, re.I)
    if match:
        name = match.group(1)
        try:
            dll = ctypes.WinDLL(name)
        except OSError as e:
            missing.append((name, str(e)))
            dll = None
        continue
    match = re.match(r'^\s{20,}[0-9a-fA-F]+ ([A-Za-z_?][A-Za-z0-9_?@$]*)\s*$', line)
    if match and dll:
        function = match.group(1)
        checked += 1
        if not loader.GetProcAddress(dll._handle, function.encode('ascii')):
            missing.append((name, function))
print({'checked': checked, 'missing': missing})
