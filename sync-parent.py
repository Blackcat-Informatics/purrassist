from pathlib import Path
import subprocess,json,hashlib,shutil
root=Path.cwd(); parent=Path('/home/paudley/Active/purrassist/.worktrees/12-tool-call-recovery-add-the-pythonic-v1')
stage=root/'.stage/tool-call-recovery-add-the-gemma3-tool/parent-sync'
stage.mkdir()
def git(*args,cwd=root):return subprocess.check_output(['git',*args],cwd=cwd)
old=git('write-tree').strip().decode(); new=git('write-tree',cwd=parent).strip().decode()
(stage/'trees.json').write_text(json.dumps({'old':old,'new':new},indent=2)+'\n')
paths=[p.decode() for p in git('diff','--name-only','-z').split(b'\0') if p]
newpaths=[p.decode() for p in git('ls-files','--others','--exclude-standard','-z').split(b'\0') if p and not p.startswith(b'.stage/')]
owned=set(paths+newpaths)
(stage/'author.patch').write_bytes(git('diff','--binary','--src-prefix=a/','--dst-prefix=b/'))
manifest={}
for name in owned:
 p=root/name
 if p.is_file():
  dest=stage/'author'/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dest)
  manifest[name]=hashlib.sha256(p.read_bytes()).hexdigest()
(stage/'author-files.json').write_text(json.dumps(manifest,indent=2)+'\n')
changed=[p.decode() for p in git('diff','--name-only','-z',old,new).split(b'\0') if p]
conflicts=[]
for name in changed:
 before=subprocess.run(['git','show',old+':'+name],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
 after=subprocess.run(['git','show',new+':'+name],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
 p=root/name
 if name not in owned:
  if after.returncode==0:
   p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(after.stdout)
  elif p.exists():p.unlink()
  continue
 assert before.returncode==0 and after.returncode==0,('new/delete overlap',name)
 current=p.read_bytes()
 if current==before.stdout:p.write_bytes(after.stdout);continue
 area=stage/'merge'/name;area.mkdir(parents=True)
 for label,data in [('current',current),('base',before.stdout),('parent',after.stdout)]: (area/label).write_bytes(data)
 result=subprocess.run(['git','merge-file','-p','--diff3','-L','Gemma authored','-L','old Pythonic baseline','-L','updated Pythonic baseline',str(area/'current'),str(area/'base'),str(area/'parent')],stdout=subprocess.PIPE)
 assert result.returncode<128,(name,result.returncode)
 p.write_bytes(result.stdout)
 if result.returncode:conflicts.append(name)
subprocess.run(['git','read-tree',new],check=True)
assert git('write-tree').strip().decode()==new
for name,digest in manifest.items():
 if name not in changed:assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
(stage/'conflicts.json').write_text(json.dumps(conflicts,indent=2)+'\n')
print(json.dumps({'authored_files_preserved':len(owned),'parent_paths_updated':len(changed),'conflicts':conflicts},indent=2))
