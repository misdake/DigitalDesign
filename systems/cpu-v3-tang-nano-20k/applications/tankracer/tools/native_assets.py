"""Normal application producer: exact trig and three frozen physical invocations."""
import argparse,hashlib,json,os,struct,subprocess,sys
from pathlib import Path

def atomic(path,data):
 path.parent.mkdir(parents=True,exist_ok=True);data=data.encode() if isinstance(data,str) else data;tmp=path.with_suffix(path.suffix+".tmp")
 with tmp.open("wb") as f:f.write(data);f.flush();os.fsync(f.fileno())
 os.replace(tmp,path);assert path.read_bytes()==data

def main():
 parser=argparse.ArgumentParser();parser.add_argument("--output",type=Path,required=True);out=parser.parse_args().output;out.mkdir(parents=True,exist_ok=True)
 reference=Path(os.environ["TANKRACER_REFERENCE"]).resolve().parent;fixture_path=Path(os.environ["TANKRACER_FIXTURES"]).resolve();fixture=json.loads(fixture_path.read_text());owner=Path(__file__).resolve().parent.parent
 subprocess.run([sys.executable,str(owner/"tools/export_trig_assets.py"),"--reference",str(reference),"--output",str(out)],check=True)
 for name in ["sine.i16.bin","angle.i16.bin","trig-assets.json"]:atomic(out/name,(out/name).read_bytes())
 records=[];description=[]
 for label,name,index in [("ordinary","launch-mixed",8),("periodic","launch-mixed",9),("airborne-boost","airborne-boost",2)]:
  case=next(c for c in fixture["cases"] if c["name"]==name);frame=case["frames"][index];previous=case["frames"][index-1]["state"];initial={k:list(v) for k,v in previous.items()};control=frame["input"]
  for i in range(4):initial["suspension"][i*6]=control["kinds"][i];initial["suspension"][i*6+1]=control["gaps"][i]
  initial["suspension"][36:38]=[0,0]
  words=[]
  def i32(values):
   for v in values:words.extend([v&65535,(v>>16)&65535])
  for k in ["attitude","suspension","drive","motion","boost"]:i32(initial[k])
  for k in ["suspension_config","drive_config"]:i32(case[k])
  words.extend(case["attitude_config"]);i32(case["pose_config"]);i32(control["environment"]);i32(control["frame"])
  assert len(words)==893
  for k in ["attitude","suspension","drive","motion","boost"]:i32(frame["state"][k])
  assert len(words)==1111;records.extend(words);assert frame["rng_before"]==frame["rng_after"]
  description.append({"label":label,"case":name,"frame":index+1,"dt":control["frame"][0],"numerics":case["attitude_config"][4],"rng_before":frame["rng_before"],"rng_after":frame["rng_after"],"initial":initial,"expected":frame["state"]})
 atomic(out/"physics_samples.bin",struct.pack("<"+"H"*len(records),*records))
 constants=["mod static_assets;use static_assets::*;\n"]
 resources=[]
 for symbol,name in [("PHYSICS_SINE","sine.i16.bin"),("PHYSICS_ANGLE","angle.i16.bin"),("PHYSICS_SAMPLES","physics_samples.bin")]:
  path=(out/name).resolve();payload=path.read_bytes();constants.append(f'#[asset] pub static {symbol}:AssetBytes=AssetBytes::new(include_bytes!({json.dumps(path.as_posix())}));\n');resources.append({"file":name,"placement":"source","symbol":symbol,"bytes":len(payload),"sha256":hashlib.sha256(payload).hexdigest()})
 constants.append("pub const PHYSICS_RECORD_WORDS:u16=1111;pub const PHYSICS_RECORDS:u16=3;\n")
 atomic(out/"physics_assets.rs","".join(constants));atomic(out/"trig_lookup.rs",(owner/"rcc/trig_asset_lookup.rs").read_bytes())
 manifest={"resources":resources,"sector_relocations":[],"samples":description,"fixture_source":{"path":str(fixture_path),"sha256":hashlib.sha256(fixture_path.read_bytes()).hexdigest()},"record_layout":{"initial":0,"suspension_config":218,"drive_config":270,"attitude_config":790,"pose_config":795,"environment":799,"frame":835,"expected":893,"words":1111}}
 atomic(out/"assets.json",json.dumps(manifest,indent=2)+"\n")
if __name__=="__main__":main()
