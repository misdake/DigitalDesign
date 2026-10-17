"""全新插值数值规范；仅数学整数/Fraction，不导入或生成任何旧I实现。"""
from dataclasses import dataclass
from fractions import Fraction
from math import log2


def rne_div(n,d):
    """整数除正整数，最近偶数；对负数严格符号对称。"""
    assert d>0
    q,r=divmod(abs(n),d);q+=2*r>d or 2*r==d and q&1
    return -q if n<0 else q


def trunc_scale(n,right):
    """右移向零；负right代表精确左移。系数取负可交换。"""
    if right<=0:return n<<-right
    return (-1 if n<0 else 1)*(abs(n)>>right)


def floor_scale(n,right):return n>>right if right>=0 else n<<-right


def normal12_from18(code):
    assert -(1<<17)<=code<(1<<17)
    return max(-2048,min(2047,rne_div(code,64)))


SEED=tuple(rne_div(1<<25,256+j) for j in range(257))
SLOPE=tuple(SEED[j]-SEED[j+1] for j in range(256))
# 浮点只在生成不可变对数表时使用；像素数值不调用浮点log。
LOG=tuple(round(log2(1+j/64)*256) for j in range(64))
N_LIMIT=2*131071**2


def reciprocal18(d):
    assert 0<d<(1<<32)
    e=d.bit_length()-1;m=floor_scale(d,e-17);j=m>>9&255;f=m&511
    correction=rne_div(f*SLOPE[j],512);r=SEED[j]-correction
    assert 65536<=r<=131072
    return dict(e=e,m=m,j=j,f=f,seed=SEED[j],slope=SLOPE[j],correction=correction,r=r)


def log_q8(n):
    assert n>0
    e=n.bit_length()-1;m=floor_scale(n,e-6)
    return e*256+LOG[m-64]


def log_dyadic_q8(v):
    v=Fraction(v);assert v>0 and v.denominator&(v.denominator-1)==0
    return log_q8(v.numerator)-256*(v.denominator.bit_length()-1)


def power2(e):return Fraction(1<<e) if e>=0 else Fraction(1,1<<-e)


@dataclass(frozen=True)
class Triangle:
    # n是canonical未乘2的Nx/Ny；R的h30已取相同canonical符号。
    n:tuple
    exps:tuple
    delta:int
    tri_shift:int
    uv:tuple=((0,0),(65535,0),(0,65535))
    normal:tuple=((0,0,1024),)*3
    rgb:tuple=((255,0,0),(0,255,0),(0,0,255))
    far_raw:int=199*65536
    tex_log:tuple=(8,8)
    max_mip:int=8
    bias_q4:int=0

    def validate(self):
        assert len(self.n)==len(self.exps)==3
        assert all(len(v)==2 and all(-131071<=x<=131071 for x in v) for v in self.n)
        assert all(0<=e<=19 for e in self.exps)
        assert all(abs(v<<e)<=N_LIMIT for row,e in zip(self.n,self.exps) for v in row),'真实P18叉积对称量化恢复的N36域'
        assert 0<self.delta<(1<<53) and 0<=self.tri_shift<=15
        assert all(len(v)==2 and all(0<=x<=65535 for x in v) for v in self.uv)
        assert all(len(v)==3 and all(-2048<=x<=2047 for x in v) for v in self.normal)
        assert all(len(v)==3 and all(0<=x<=255 for x in v) for v in self.rgb)
        assert 0<self.far_raw<200*65536
        assert len(self.tex_log)==2 and all(0<=v<=31 for v in self.tex_log)
        assert 0<=self.max_mip<=31 and -128<=self.bias_q4<=127


def lane_h(t,h00,lane):
    assert len(h00)==3 and 0<=lane<4
    x,y=lane&1,lane>>1
    # 必须在原边尺度加步长后才对齐；禁止先截步长。
    return tuple(h+2*nn[0]*x+2*nn[1]*y for h,nn in zip(h00,t.n))


def align_h(h,exps,guard=0):
    assert guard in (0,1) and len(h)==len(exps)==3
    assert all(0<=v<(1<<29) for v in h),'只允许R mask接纳的非负h30进入I'
    E=max(exps)-guard
    aligned=tuple(floor_scale(v,E-e) for v,e in zip(h,exps));d=sum(aligned)
    assert d<(1<<(31+guard))
    exact=sum(Fraction(v)*power2(e-E) for v,e in zip(h,exps))
    assert 0<=exact-d<2,'至少一条max-exp边是整数，其余至多两条各损失不足1'
    return dict(E=E,h=aligned,d=d,exact_common=exact,discarded=exact-d,zero=d==0)


def weights16(h,d):
    # 数值零D保留可确定的几何anchor算术，不授权发布该lane。
    if d==0:return dict(weights=(0,0,65536),hm=(0,0),raw=(0,0),**reciprocal18(1))
    rc=reciprocal18(d);hm=tuple(floor_scale(v,rc['e']-16) for v in h[:2])
    raw=tuple(rc['r']*v>>17 for v in hm);w0=min(65536,raw[0]);w1=min(65536-w0,raw[1])
    assert 0<=w0<=65536 and 0<=w1<=65536-w0
    return dict(weights=(w0,w1,65536-w0-w1),hm=hm,raw=raw,**rc)


def attribute(values,w,rounding='half_up'):
    assert sum(w)==65536 and min(w)>=0
    direct=sum(a*b for a,b in zip(values,w))
    delta=(values[2]<<16)+w[0]*(values[0]-values[2])+w[1]*(values[1]-values[2])
    assert direct==delta,'两差分等式必须保留完整乘积，只在最终出口舍入一次'
    if rounding=='rne':return rne_div(delta,65536)
    if rounding=='half_up':return (delta+32768)//65536
    assert rounding=='floor';return delta>>16


def depth(t,alignment,rc):
    ed=t.delta.bit_length()-1;K=floor_scale(t.delta,ed-30)
    P=rc['r']*K;right=rc['e']+alignment['E']+38-ed-t.tri_shift
    full=floor_scale(P,right);overflow=full>=(1<<32)
    valid=not alignment['zero'] and not overflow and full<(1<<24) and (full>>8)<=(t.far_raw>>8)
    return dict(delta_e=ed,K31=K,product=P,right=right,Z_full=full,Z32=full&0xffffffff,Z16=(full>>8)&65535,overflow=overflow,valid=valid)


def pixel(t,h,guard=0,rounding='half_up'):
    t.validate();a=align_h(h,t.exps,guard);w=weights16(a['h'],a['d']);ww=w['weights']
    uv=tuple(attribute(v,ww,rounding) for v in zip(*t.uv))
    normal=tuple(attribute(v,ww,rounding) for v in zip(*t.normal))
    rgb=tuple(attribute(v,ww,rounding) for v in zip(*t.rgb))
    assert all(0<=v<=65535 for v in uv) and all(-2048<=v<=2047 for v in normal) and all(0<=v<=255 for v in rgb)
    return dict(alignment=a,reciprocal=w,weights=ww,uv=uv,normal=normal,rgb=rgb,**depth(t,a,w))


def signed(n,bits):
    n&=(1<<bits)-1
    return n-(1<<bits) if n>>(bits-1) else n


def decompose_minus_s(S):
    """独立通用S38分解；不先在38位容器里对最小负数取负。"""
    assert -(1<<37)<=S<(1<<37)
    B=-S;lo=signed(B,36);q=(B-lo)//(1<<36)
    assert B==lo+q*(1<<36) and -2<=q<=2
    return dict(B=B,Blo=lo,q=q)


def wide_dsp(G,u,S):
    """u*Blo+C的54位模数等式；q*u只含0/±u/±2u。"""
    assert -(1<<53)<=G<(1<<53) and 0<=u<=65535
    b=decompose_minus_s(S);hi=((G>>36)+b['q']*u)&((1<<18)-1);lo=G&((1<<36)-1)
    C=(hi<<36)|lo;raw=(u*b['Blo']+C)&((1<<54)-1)
    direct=G-u*S
    assert raw==direct&((1<<54)-1)
    return dict(**b,Chi=hi,Clow=lo,C54=C,raw54=raw,value=signed(raw,54),direct=direct)


def lod_coefficients(t,mode='exact'):
    if mode=='exact':
        n=tuple(tuple(v<<e for v in row) for row,e in zip(t.n,t.exps))
        S=tuple(sum(row[axis] for row in n) for axis in range(2))
        G=tuple(sum(t.uv[i][channel]*n[i][axis] for i in range(3)) for channel in range(2) for axis in range(2))
        assert all(abs(v)<=N_LIMIT for row in n for v in row)
        assert all(-(1<<37)<=v<(1<<37) for v in S) and all(-(1<<53)<=v<(1<<53) for v in G)
        assert all(decompose_minus_s(v)['q'] in (-1,0,1) for v in S)
        return dict(Nc=0,n=n,S=S,G=G,all_zero=not any(v for row in n for v in row),lost_coefficients=0,bits=54)
    nonzero=[e+abs(v).bit_length()-1 for e,row in zip(t.exps,t.n) for v in row if v]
    Nc=max(nonzero)-16 if nonzero else 0
    if mode=='e':Nc=max(t.exps)
    assert mode in ('nc','e')
    n=tuple(tuple(trunc_scale(v,Nc-e) for v in row) for e,row in zip(t.exps,t.n))
    assert all(abs(v)<1<<17 for row in n for v in row)
    S=tuple(sum(row[axis] for row in n) for axis in range(2))
    G=tuple(sum(t.uv[i][channel]*n[i][axis] for i in range(3)) for channel in range(2) for axis in range(2))
    assert all(-(1<<19)<=v<(1<<19) for v in S)
    assert all(-(1<<35)<=v<(1<<35) for v in G)
    lost=sum(v!=0 and q==0 for row,qr in zip(t.n,n) for v,q in zip(row,qr))
    return dict(Nc=Nc,n=n,S=S,G=G,all_zero=not nonzero,lost_coefficients=lost,bits=36)


def derivatives(t,u,coeff):
    r=tuple(coeff['G'][2*c+axis]-u[c]*coeff['S'][axis] for c in range(2) for axis in range(2))
    direct=tuple(sum((t.uv[i][c]-u[c])*coeff['n'][i][axis] for i in range(3)) for c in range(2) for axis in range(2))
    limit=coeff['bits']-1
    assert r==direct and all(-(1<<limit)<=v<(1<<limit) for v in r)
    exact=tuple(sum(Fraction(t.uv[i][c]-u[c])*t.n[i][axis]*power2(t.exps[i]-coeff['Nc']) for i in range(3)) for c in range(2) for axis in range(2))
    assert all(abs(a-b)<3*65535 for a,b in zip(exact,r))
    dsp=tuple(wide_dsp(coeff['G'][2*c+axis],u[c],coeff['S'][axis]) for c in range(2) for axis in range(2)) if coeff['bits']==54 else ()
    assert not dsp or tuple(p['value'] for p in dsp)==r
    return dict(r=r,exact=exact,dsp=dsp)


def lod(t,anchor,mode='exact'):
    c=lod_coefficients(t,mode);r=derivatives(t,anchor['uv'],c);d=max(1,anchor['alignment']['d'])
    logs=[log_q8(abs(v))+256*t.tex_log[j//2] for j,v in enumerate(r['r']) if v]
    correction=256*(c['Nc']-anchor['alignment']['E'])-15*256+128+16*t.bias_q4
    raw=max(logs)-log_q8(d)+correction if logs else 0
    q8=max(0,min(t.max_mip*256,raw))
    return dict(coeff=c,derivative=r,raw_q8=raw,LOD_q8=q8,LOD_q3=q8>>5)


def lod_dyadic(t,anchor):
    E=anchor['alignment']['E'];d=max(1,anchor['alignment']['d']);u=anchor['uv']
    r=tuple(sum(Fraction(t.uv[i][channel]-u[channel])*t.n[i][axis]*power2(t.exps[i]-E) for i in range(3)) for channel in range(2) for axis in range(2))
    logs=[log_dyadic_q8(abs(v))+256*t.tex_log[j//2] for j,v in enumerate(r) if v]
    raw=max(logs)-log_q8(d)-15*256+128+16*t.bias_q4 if logs else 0
    q8=max(0,min(t.max_mip*256,raw))
    return dict(r=r,raw_q8=raw,LOD_q8=q8,LOD_q3=q8>>5)


def quad(t,h00,mask,guard=0,rounding='half_up',lod_mode='e'):
    assert 0<mask<16
    ps={i:pixel(t,lane_h(t,h00,i),guard,rounding) for i in range(4) if mask>>i&1}
    anchor=next(i for i in range(4) if mask>>i&1)
    return dict(mask=mask,anchor=anchor,pixels=ps,passed=sum(p['valid']<<i for i,p in ps.items()),lod=lod(t,ps[anchor],lod_mode),lod_nc=lod(t,ps[anchor],'nc'),lod_direct_e=lod(t,ps[anchor],'e'),lod_dyadic=lod_dyadic(t,ps[anchor]))
