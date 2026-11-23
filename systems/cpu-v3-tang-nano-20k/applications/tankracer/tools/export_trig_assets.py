"""Export the exact reference initializer constants; never regenerate or mirror them."""
import argparse,hashlib,json,os,re,struct
from pathlib import Path

def atomic(path,data):
 path.parent.mkdir(parents=True,exist_ok=True);tmp=path.with_suffix(path.suffix+".tmp");tmp.write_bytes(data);os.replace(tmp,path);assert path.read_bytes()==data

def main():
 a=argparse.ArgumentParser();a.add_argument("--reference",type=Path,required=True);a.add_argument("--output",type=Path,required=True);args=a.parse_args();ref=args.reference;out=args.output
 loader=ref/"src/frame_math.c";header=ref/"src/frame_math_tables.h";text=header.read_text();assert "i<20480u" in loader.read_text()
 records=[]
 for name,count,file in [("math_sine",20480,"sine.i16.bin"),("math_angle",257,"angle.i16.bin")]:
  match=re.search(r"static const int32_t "+name+r"\["+str(count)+r"\]=\{([\s\S]*?)\};",text);assert match,name
  values=[int(v.strip()) for v in match[1].split(",") if v.strip()];assert len(values)==count;assert all(-32768<=v<=32767 for v in values)
  data=struct.pack("<"+"h"*count,*values);atomic(out/file,data)
  rust=ref/"crates/game/data"/("sine.bin" if name=="math_sine" else "angle.bin");assert data==rust.read_bytes(),"independent reference Rust loader table mismatch"
  records.append({"name":name,"entries":count,"source_i32_bytes":count*4,"i16_bytes":len(data),"minimum":min(values),"maximum":max(values),"negative_entries":sum(v<0 for v in values),"first":values[0],"last":values[-1],"sha256":hashlib.sha256(data).hexdigest(),"rust_loader_bytes_equal":True,"filename":file})
 source=[loader,header,ref/"tools/export_rust_math.mjs",ref/"crates/game/src/math.rs"]
 manifest={"source_kind":"Uncompressed int32 constants read directly from the C reference frame_math_initializer's included header. Upstream header is generated/qualified; this exporter copies its committed integers and does not run its formula generator.","source_files":[{"path":str(p.resolve()),"bytes":len(p.read_bytes()),"sha256":hashlib.sha256(p.read_bytes()).hexdigest()} for p in source],"tables":records,"total_exact_bytes":sum(r["i16_bytes"] for r in records),"single_32k_window":False,"correction":"The source has 20480 sine i32 entries, not 10240. Its 40960-byte Rust file is already the exact i16 representation.","table_policy":"All source entries, cosine tail, terminal angle alias and non-mirrored values retained. No transcendentals, approximation, delta encoding, compression or fixture-derived production data."}
 atomic(out/"trig-assets.json",(json.dumps(manifest,indent=2)+"\n").encode())
 print(json.dumps(manifest,indent=2))
if __name__=="__main__":main()
