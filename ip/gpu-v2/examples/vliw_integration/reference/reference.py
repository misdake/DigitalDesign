"""独立整数参考：直接行列式/叉乘，不读取RTL或控制ROM。"""
MASK32=(1<<32)-1
MASK54=(1<<54)-1
def signed(x, width):
    x &= (1 << width)-1
    return x-(1 << width) if x >> (width-1) else x


def oracle(uniform, config, vertices):
    """Mathematical dot products with the explicit modular output ABI."""
    bits = 18 if config[3] & 256 else 16
    output = []
    for p, n, attr in vertices:
        xyz = [signed(config[k]+(((p >> (10*k)) & 1023) << (config[3] & 31)), 32) for k in range(3)]
        normal = [signed(n >> (10*k), 10)*128 for k in range(3)]
        pos = [(sum(xyz[k]*signed(uniform[4*j+k],32) for k in range(3))
                + 65536*signed(uniform[4*j+3],32)) >> 16 & MASK32 for j in range(3)]
        nv = [signed(sum(normal[k]*signed(uniform[12+3*j+k],32) for k in range(3))
                     >> (18 if bits==16 else 16), bits) & ((1<<18)-1) for j in range(3)]
        output += pos + [nv[0] | (nv[1]<<18), nv[2], attr & MASK32]
    return output
