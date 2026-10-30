# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
# Why not Rust: this capture composes the existing Python qualification module without reimplementing its transport or fixture logic.
from pathlib import Path
import importlib.util, shutil, json
root = Path.cwd()
stage = root / ".stage/tool-call-recovery-add-the-gemma3-tool/cli-capture"
stage.mkdir()
spec = importlib.util.spec_from_file_location("verify", root / "scripts/verify-published.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)
fixtures = stage / "fixtures"
shutil.copytree(root / "scripts/data/verify-published", fixtures)
binary = verify.build_workspace_cli(root)
(stage / "binary.txt").write_text(str(binary))
for name in ["capture-root", "replay-root"]:
    (stage / name).mkdir()
verify.capture_fixtures(binary, stage / "capture-root", fixtures)
errors = verify.run_mcp_transcript(binary, stage / "replay-root", fixtures)
errors += verify.run_call_transcript(binary, stage / "replay-root", fixtures)
if errors:
    raise SystemExit("\n".join(errors))
changes = []
def compare(old, new, path):
    if old == new: return
    if isinstance(old, dict) and isinstance(new, dict) and old.keys() == new.keys():
        for key in old: compare(old[key], new[key], path + "/" + key)
    elif isinstance(old, list) and isinstance(new, list) and len(old) == len(new):
        for index, (a,b) in enumerate(zip(old,new)): compare(a,b,path+"/"+str(index))
    elif isinstance(old,str) and isinstance(new,str):
        try: a,b=json.loads(old),json.loads(new)
        except ValueError: changes.append([path,old,new])
        else: compare(a,b,path+"/decoded")
    else: changes.append([path,old,new])
for name in ["call-response.json", "mcp-responses.jsonl"]:
    old=(root/"scripts/data/verify-published"/name).read_text()
    new=(fixtures/name).read_text()
    if name.endswith("jsonl"):
        compare([json.loads(x) for x in old.splitlines()], [json.loads(x) for x in new.splitlines()], name)
    else: compare(json.loads(old),json.loads(new),name)
(stage/"deltas.json").write_text(json.dumps(changes,indent=2)+"\n")
print("Exact replay passed; changed fields:",len(changes))
print(json.dumps(changes,indent=2))
