import re
def wrapper(rtl):
    start = re.search(r'^module\s+(\w+)', rtl, re.M)
    assert start, 'missing top module'
    module = start.group(1)
    head = rtl[start.end():rtl.index(');', start.end())]
    ports = []
    pattern = r'\b(input|output)\s+(?:(wire|reg)\s*)?(?:(signed)\s*)?(\[\s*\d+\s*:\s*0\s*\]\s*)?([^;]*?)(?=\binput\b|\boutput\b|$)'
    for m in re.finditer(pattern, head):
        direction, _, sign, width, names = m.groups()
        bits = int(re.search(r'\d+', width).group()) + 1 if width else 1
        for name in names.split(','):
            name = name.strip()
            if not name: continue
            assert re.fullmatch(r'\w+', name), name
            ports.append(dict(direction=direction, name=name, bits=bits, signed=bool(sign)))
    assert len({p['name'] for p in ports}) == len(ports)
    control = {'clk', 'ce', 'reset'}
    assert control.issubset({p['name'] for p in ports if p['direction'] == 'input'})
    inputs = [p for p in ports if p['direction'] == 'input' and p['name'] not in control]
    outputs = [p for p in ports if p['direction'] == 'output']
    iw, ow = sum(p['bits'] for p in inputs), sum(p['bits'] for p in outputs)
    assert iw > 0 and ow > 0
    body = f'module timing_top(input clk,ce,reset,serial_in,output [5:0] leds);\nwire [{iw-1}:0] src;\nwire [{ow-1}:0] sink;\ngenvar i;\ngenerate for(i=0;i<{iw};i=i+1)begin:source_boundary\n'
    body += 'if(i==0)DFFE ff(.Q(src[i]),.D(serial_in),.CLK(clk),.CE(ce));\nelse DFFE ff(.Q(src[i]),.D(src[i-1]),.CLK(clk),.CE(ce));\nend endgenerate\n'
    connections = ['.clk(clk)', '.ce(ce)', '.reset(reset)']
    for group, bus in ((inputs, 'src'), (outputs, 'sink')):
        offset = 0
        for p in group:
            connections.append(f'.{p["name"]}({bus}[{offset} +: {p["bits"]}])')
            offset += p['bits']
    body += module + ' dut(' + ','.join(connections) + ');\n'
    for j in range(6):
        taps = ','.join(f'sink[{k}]' for k in range(j, ow, 6))
        expr = f'^{{{taps}}}' if taps else "1'b0"
        body += f'DFFE observe{j}(.Q(leds[{j}]),.D({expr}),.CLK(clk),.CE(ce));\n'
    return body + 'endmodule\n', dict(input_bits=iw, output_bits=ow, ports=ports,
        registered_wrapper_ff=iw+6, scope='all non-clock/reset/CE inputs registered; six registered xor observers')
