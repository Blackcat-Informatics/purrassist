# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
import re, subprocess, json, sys
from pathlib import Path
base=Path(__file__).resolve().parent
counts={}
def unique(items):
    out={}
    for k,v in items:
        if k in out: raise ValueError("duplicate JSON key: "+k)
        out[k]=v
    return out
for doc in sys.argv[1:]:
    source=Path(doc).read_text()
    for i,(kind,body) in enumerate(re.findall(r'^```([^\n]*)\n(.*?)^```',source,re.M|re.S)):
        if kind not in {"turtle","srl","json","apparmor","gbnf"}: continue
        counts[kind]=counts.get(kind,0)+1
        target=base/f'{Path(doc).stem}-{i}.{kind}'
        target.write_text(body)
        if kind=='json':
            json.loads(body,object_pairs_hook=unique,parse_constant=lambda x: (_ for _ in ()).throw(ValueError(x)))
        elif kind in {"apparmor","gbnf"}:
            cmd=['apparmor_parser','-QK',str(target)] if kind=='apparmor' else [str(base/'gbnf-probe'),str(target)]
            subprocess.run(cmd,check=True)
        print(doc,kind,i,"PASS")
    subprocess.run([str(base/'probe/target/debug/toolgraph-spec-probe'),doc],check=True)
print(json.dumps(counts,sort_keys=True))
