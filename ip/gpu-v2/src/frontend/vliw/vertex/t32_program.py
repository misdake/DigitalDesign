"""Bounded-F16 triangle calendar in the existing 512x72 V/T control store.

Exact cross54 components are reduced to a shared-exponent signed27 vector
before the original MAC computes determinant54 and viewport coefficients.
Temporary rows retain three guard bits and a coarse exponent; a second reduction
is equivalent to reducing once by the common exponent. V/bounds words stay intact.
"""
from pathlib import Path
import re

# T keeps the existing V RF/MAC bit positions. Independent fields compose a cycle.
def assemble():
    code=[]
    fields=dict(negwrite=(2,1),bsrc=(3,1),axis=(4,2),norm=(6,3),sticky=(9,1),
        vertex=(9,2),qreset=(11,1),up=(12,2),qmaxreset=(14,1),constant=(15,2),
        dst=(17,4),a=(21,4),b=(25,4),write=(29,2),mul=(31,1),cap=(32,2),
        detinit=(34,1),outsel=(35,3),flow=(40,3),uphi=(50,1),read=(53,1),
        wr=(54,1),ra=(55,4),pr=(59,1),wa=(60,3),row=(63,4),out=(67,1),
        conditional=(68,1),align=(69,1),aligned=(70,1))
    def emit(**kw):
        i=len(code);word=0
        for k,v in kw.items():
            shift,width=fields[k];assert 0<=v<1<<width,(k,v)
            word|=v<<shift
        code.append(word)
    # The position BSRAM Q and unpack register are independent pipeline stages.
    # Fill two otherwise empty RF write slots with constants before P writes.
    negative_rf=[10,11,15]
    for t in range(13):
        kw={}
        if t<9:kw.update(pr=1,vertex=t//3,axis=t%3)
        elif t<12:kw.update(pr=1,vertex=2,axis=t-9)
        if 1<=t<=9:kw.update(uphi=1)
        if 10<=t<=12:kw.update(uphi=1,up=1)
        if 2<=t<=10:kw.update(write=1,dst=t-1)
        if 11<=t<=12:kw.update(write=1,dst=negative_rf[t-11],negwrite=1)
        if t==10:kw.update(flow=2,mul=1,a=4,b=8,bsrc=1,qmaxreset=1)
        if t==11:kw.update(cap=1)
        if t==12:kw.update(mul=1,a=5,b=10,bsrc=1)
        if t==0:kw.update(write=3,dst=12,constant=0)
        if t==1:kw.update(write=3,dst=13,constant=1)
        emit(**kw)
    for edge,(u,v) in enumerate([(1,2),(2,0),(0,1)]):
        # Pj*Pk - Pk*Pj. Capture of product 1 overlaps issue of product 2.
        pairs=[(1,2),(2,0),(0,1)]
        order=[2,0,1] if edge==0 else [0,1,2]
        for step,component in enumerate(order):
            j,k=pairs[component]
            # The first edge's two z products were issued in the P-read tail;
            # later edges already have product 1 from the previous C return.
            if step==0 and edge!=0:
                emit(mul=1,a=1+3*u+k,b=negative_rf[j],bsrc=1,cap=1)
            kw=dict(cap=2,sticky=1,qreset=1)
            if edge==0 and step==0:kw.update(write=1,dst=15,negwrite=1)
            if step<2:
                nj,nk=pairs[order[step+1]];kw.update(mul=1,a=1+3*u+nj,b=1+3*v+nk,bsrc=1)
            emit(**kw)
            kw=dict(norm=1,wr=1,wa=component,conditional=1)
            if edge==0 and step==0:kw.update(write=3,dst=14,constant=2)
            if step<2:
                nj,nk=pairs[order[step+1]]
                kw.update(mul=1,a=1+3*u+nk,b=negative_rf[nj],bsrc=1,cap=1)
            else:kw.update(read=1,ra=0)
            emit(**kw)
        # A=2^(26-(qmax-qi)); B=Ni. The original DSP returns the
        # exact common-scale toward-zero mantissa in bits [52:26].
        # Read next / issue current / write previous overlap, with one sign bit.
        for j in range(3):
            kw=dict(mul=1,align=1,bsrc=0)
            if edge<2:
                kw.update(pr=1,vertex=edge,axis=j)
                if j:kw.update(uphi=1,up=1)
                if j==2:kw.update(write=1,dst=10,negwrite=1)
            if j<2:kw.update(read=1,ra=j+1)
            else:kw.update(read=1,ra=0)
            if j:kw.update(aligned=1,wr=1,wa=j-1)
            emit(**kw)
        # Read0 is already returned. Retire the final alignment while issuing
        # the first determinant/C term; C input is sampled only at retirement.
        if edge==0:
            emit(aligned=1,wr=1,wa=2,mul=1,a=1,bsrc=0,read=1,ra=1,write=1,dst=11,negwrite=1,uphi=1,up=1)
            emit(mul=1,a=2,bsrc=0,read=1,ra=2,cap=1,write=1,dst=15,negwrite=1)
            emit(mul=1,a=3,bsrc=0,cap=2)
            emit(cap=2,detinit=1,sticky=1)
            emit(norm=3,flow=5)
            emit(norm=4)
            emit(out=1,row=9,outsel=3,read=1,ra=0)
        # NX/NY use the already returned work Q while the same operand feeds C.
        kw=dict(mul=1,a=12,bsrc=0,read=1,ra=1,out=1,row=3*edge,outsel=1)
        if edge:kw.update(aligned=1,wr=1,wa=2)
        if edge==1:kw.update(write=1,dst=11,negwrite=1,uphi=1,up=1)
        emit(**kw)
        kw=dict(mul=1,a=13,bsrc=0,read=1,ra=2,cap=1,out=1,row=3*edge+1,outsel=2)
        if edge==1:kw.update(write=1,dst=15,negwrite=1)
        emit(**kw)
        emit(mul=1,a=14,bsrc=0,cap=2)
        kw=dict(out=1,row=3*edge+2,outsel=4)
        if edge<2:
            nu,nv=[(1,2),(2,0),(0,1)][edge+1]
            kw.update(mul=1,a=1+3*nu+1,b=1+3*nv+2,bsrc=1,qmaxreset=1)
        emit(**kw)
    emit(flow=6)
    addresses=list(range(128,256))
    assert len(code)<=len(addresses),len(code)
    return dict(zip(addresses,code))

def update():
    path=Path(__file__).with_name('control_rom.v')
    source=path.read_text()
    pairs={}
    for address,expression in re.findall(r"mem\[(\d+)\]=(.*?);",source):
        values=re.findall(r"72'h([0-9a-f]+)",expression)
        if 'SP_COMPACT' in expression:
            pairs[int(address)]=(int(values[0],16),int(values[1],16))
        else:
            pairs[int(address)]=(int(values[0],16),)*2
    assert len(pairs)==512,len(pairs)
    scheduled=assemble()
    program={a:0 for a in list(range(128,256))+list(range(320,512))}
    program.update(scheduled)
    for a,value in program.items():pairs[a]=(value,value)
    used=0
    for c,d in pairs.values():used |= c
    source=re.sub(r"localparam \[71:0\] COMPACT_USED_BITS=72'h[0-9a-f]+;",
                  f"localparam [71:0] COMPACT_USED_BITS=72'h{used:018x};",source)
    def row(m):
        a=int(m[1]);c,d=pairs[a]
        value=f"72'h{c:018x}" if c==d else f"SP_COMPACT ? 72'h{c:018x} : 72'h{d:018x}"
        return f"mem[{a}]={value};"
    source=re.sub(r"mem\[(\d+)\]=.*?;",row,source)
    bank=-1
    def init(m):
        nonlocal bank
        i=int(m[1],16)
        if i==0:bank+=1
        a=i*8
        c=sum(((pairs[a+j][0]>>(36*bank))&((1<<36)-1))<<(36*j) for j in range(8))
        d=sum(((pairs[a+j][1]>>(36*bank))&((1<<36)-1))<<(36*j) for j in range(8))
        value=f"288'h{c:072x}" if c==d else f"SP_COMPACT ? 288'h{c:072x} : 288'h{d:072x}"
        return f".INIT_RAM_{i:02X}({value})"
    source=re.sub(r"\.INIT_RAM_([0-9A-F]{2})\([^)]*\)",init,source)
    assert bank==1
    path.write_text(source)
    print(f'Bounded-F16 T calendar: {len(scheduled)} slots; legacy T removed; V/bounds unchanged')

if __name__=='__main__':update()
