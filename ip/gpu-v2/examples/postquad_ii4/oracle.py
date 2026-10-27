"""Captured independent integer Color/Final/ROP helpers; no DUT scheduling answers."""
import re
options=["final8"]
def values(path):
 return [int(x,16) if 'x' not in x.lower() else None for x in re.sub(r'//[^\n]*','',path.read_text()).split()]

def shade(tint,tex,spec,g,h):
 rgb=[]
 for k in range(3):
  a=(tint>>(8*k))&255;b=(tex>>(8*k))&255;c=(spec>>(8*k))&255
  n=a*b+128
  base=(n if 'tint256' in options else n+(n>>8))>>8
  if 'final8' in options: v=(base*g>>8)+(c*h>>8)
  else:
   n=(base*g>>7)+(c*h>>7)
   v=(n>>1)+int((n&3)==3)
  rgb.append(min(255,v))
 return sum(v<<(8*k) for k,v in enumerate(rgb))

def framebuffer(rows,rgb,initial,tb):
 mem=bytearray(initial)
 alpha=int(re.search(r'alpha=(\d+)',tb)[1]);blend=int(re.search(r'blend=(\d+)',tb)[1])
 func=int(re.search(r'depth_func=(\d+)',tb)[1]);dw=int(re.search(r'depth_write=(\d+)',tb)[1])
 for j in range(len(rows)//8):
  head=rows[j*8]>>104;x=((head>>8)&255)*2;y=((head>>16)&127)*2;mask=head&15
  for lane in range(4):
   xx=x+(lane&1);yy=y+(lane>>1)
   offset=(yy//16*32+xx//16)*512+(yy%16)*32+(xx%16)*2
   ca=32768+offset;da=49152+offset
   assert ca+1<len(mem) and da+1<len(mem)
   old=int.from_bytes(mem[ca:ca+2],'little');od=int.from_bytes(mem[da:da+2],'little')
   depth=(rows[j*8+2*lane+1]>>72)&65535
   passed=[False,depth<od,depth==od,depth<=od,depth>od,depth!=od,depth>=od,True][func]
   if not(mask&(1<<lane)) or not passed: continue
   color=rgb[j*4+lane];assert color is not None,(j,lane)
   src=[(color>>(8*k))&255 for k in range(3)]
   q=[old>>11,(old>>5)&63,old&31]
   dst=[q[0]*8+q[0]//4,q[1]*4+q[1]//16,q[2]*8+q[2]//4]
   w=(alpha+2)//4 if blend else 64
   out=[(d*64+(s-d)*w+32)//64 for s,d in zip(src,dst)]
   quant=[a>>b for a,b in zip(out,[3,2,3])] if 'q565' in options else [(a*n+127)//255 for a,n in zip(out,[31,63,31])]
   packed=quant[0]<<11|quant[1]<<5|quant[2]
   mem[ca:ca+2]=packed.to_bytes(2,'little')
   if dw:mem[da:da+2]=depth.to_bytes(2,'little')
 return mem
