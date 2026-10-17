"""Collect pinned Cargo package notices, including embedded asset notices."""
import json
from pathlib import Path
import shutil
import subprocess
root=Path(__file__).resolve().parents[1]
metadata=json.loads(subprocess.check_output(["cargo","metadata","--locked","--format-version","1"],cwd=root))
dest=root/"licenses"/"dependencies"
dest.mkdir(parents=True,exist_ok=True)
rows=[]
for package in sorted(metadata["packages"],key=lambda p:(p["name"],p["version"])):
    if package["name"]=="svbb":
        continue
    src=Path(package["manifest_path"]).parent
    notices=[]
    for f in src.rglob("*"):
        if f.is_file() and any(f.name.upper().startswith(prefix) for prefix in ["LICENSE","LICENCE","COPYING","NOTICE","OFL"]):
            rel=f.relative_to(src)
            target=dest/(package["name"]+"-"+package["version"])/rel
            target.parent.mkdir(parents=True,exist_ok=True)
            shutil.copyfile(f,target)
            notices.append(str(rel))
    rows.append("\t".join([package["name"],package["version"],package.get("license") or "SEE LICENSE FILE",package.get("repository") or "",", ".join(notices)]))
(root/"licenses"/"cargo-dependencies.tsv").write_text("name\tversion\tSPDX\trepository\tnotices\n"+"\n".join(rows),encoding="utf-8")
print(f"Collected notices for {len(rows)} packages (all target/dev dependencies included).")
