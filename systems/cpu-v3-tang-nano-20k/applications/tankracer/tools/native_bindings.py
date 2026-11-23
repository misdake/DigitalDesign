"""Bind a verified ordinary application snapshot to passive native observations."""
import argparse,hashlib,json,os,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[3]/"tools"))
from application_artifact import Artifact

def main():
 p=argparse.ArgumentParser();p.add_argument("artifact",type=Path);p.add_argument("output",type=Path);a=p.parse_args();artifact=Artifact(a.artifact);names=["PHYSICS_PHASE","PHYSICS_SAMPLE","PHYSICS_CHECKS","PHYSICS_FAILURE","PHYSICS_SINE","PHYSICS_ANGLE"];symbols={n:artifact.symbol(n,"global") for n in names};samples=json.loads((a.artifact/"tankracer-physics-assets/assets.json").read_text())["samples"]
 data={"artifact_id":artifact.manifest["artifact_id"],"package":artifact.manifest["package"],"package_sha256":hashlib.sha256(artifact.files[artifact.manifest["package"]]).hexdigest(),"symbols":symbols,"samples":samples};b=(json.dumps(data,indent=2)+"\n").encode();a.output.parent.mkdir(parents=True,exist_ok=True);tmp=a.output.with_suffix(".json.tmp")
 with tmp.open("wb") as f:f.write(b);f.flush();os.fsync(f.fileno())
 os.replace(tmp,a.output);assert a.output.read_bytes()==b
if __name__=="__main__":main()
