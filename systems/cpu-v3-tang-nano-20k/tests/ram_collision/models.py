"""Test-only native RAM bindings; initialization is derived from exported RTL."""
import hashlib
import re
from pathlib import Path


HERE = Path(__file__).resolve().parent


def header(source):
    return source[:source.index(");") + 2] + "\n"


def image(source, array, expected_digest):
    words = [0] * 512
    entries = re.findall(rf"{array}\[(\d+)\]\s*=\s*32'h([0-9a-fA-F]+)", source)
    assert entries, f"no initialization for {array}"
    for index, value in entries:
        words[int(index)] = int(value, 16)
    payload = b"".join(word.to_bytes(4, "little") for word in words)
    assert hashlib.sha256(payload).hexdigest() == expected_digest, array
    return {
        f"INIT_RAM_{i:02X}": f"256'h{int.from_bytes(payload[i*32:(i+1)*32], 'little'):064x}"
        for i in range(64)
    }


def sdp(name, params, write_clock, write_enable, write_address, write_data,
        read_clock, read_enable, read_address, output):
    parameters = ",".join(f".{key}({value})" for key, value in params.items())
    return f"""SDPB #({parameters}) {name}(
 .CLKA({write_clock}),.CEA({write_enable}),.RESETA(1'b0),
 .CLKB({read_clock}),.CEB({read_enable}),.RESETB(1'b0),.OCE(1'b0),
 .BLKSELA(3'b000),.BLKSELB(3'b000),.DI({write_data}),
 .ADA({{{write_address},1'b0,4'b1111}}),
 .ADB({{{read_address},5'b00000}}),.DO({output}));
"""


def rf(source, reference):
    text = header(source) + """reg audit_taint_a=0,audit_taint_b=0;
wire[31:0]raw_a,raw_b;
wire[31:0]read_a_data_r=($test$plusargs("POISON")&&audit_taint_a)?32'hxxxxxxxx:raw_a;
wire[31:0]read_b_data_r=($test$plusargs("POISON")&&audit_taint_b)?32'hxxxxxxxx:raw_b;
assign read_a_data=read_a_data_r;assign read_b_data=read_b_data_r;
"""
    for i, port in enumerate("ab"):
        init = reference["initializations"][f"fpu.mirror_{i}"]["sha256_le32"]
        params = reference["sdpb"] | image(source, f"mirror_{i}", init)
        text += sdp(f"mirror_{i}", params, "clk", "write_enable", "write_address",
                    "write_data", "clk", "1'b1" if port == "a" else "read_b_enable",
                    f"read_{port}_address", f"raw_{port}")
    return text + (HERE / "rf_checks.vh").read_text()


def display(source, reference):
    text = """`timescale 1ns/1ps
module DisplayLineBuffer(input write_clock,write_enable,input[8:0]write_address,
 input[63:0]write_data,input read_clock,input[8:0]read_address_a,read_address_b,
 output[31:0]read_data_a,read_data_b);
wire[31:0]raw_a,raw_b;reg audit_taint_a=0,audit_taint_b=0;
assign read_data_a=($test$plusargs("POISON")&&audit_taint_a)?32'hxxxxxxxx:raw_a;
assign read_data_b=($test$plusargs("POISON")&&audit_taint_b)?32'hxxxxxxxx:raw_b;
""" + (HERE / "display_ram_checks.vh").read_text()
    for port, hi, lo in [("a", 31, 0), ("b", 63, 32)]:
        init = reference["initializations"][f"display.memory_{port}"]["sha256_le32"]
        params = reference["sdpb"] | image(source, f"memory_{port}", init)
        text += sdp(f"ram_{port}", params, "write_clock", "write_enable",
                    "write_address", f"write_data[{hi}:{lo}]", "read_clock",
                    "1'b1", f"read_address_{port}", f"raw_{port}")
    return text + "endmodule\n"


def cache(source, vendor_body, reference):
    # Preserve the existing native binding and fail if its audited modes change.
    for key, value in reference["dpb"].items():
        assert f".{key}({value})" in vendor_body, key
    text = header(source)
    for bank in range(2):
        for port in "ab":
            q = f"bank_{bank}_{port}_read_data"
            vendor_body = vendor_body.replace(f".DO{port.upper()}({q})",
                                               f".DO{port.upper()}(raw_{bank}_{port})")
            text += f"""wire[15:0]raw_{bank}_{port};reg audit_taint_{bank}_{port}=0;
assign {q}=($test$plusargs("POISON")&&audit_taint_{bank}_{port})?16'hxxxx:raw_{bank}_{port};
"""
    text += "integer audit_collisions=0;\nalways @(posedge clk)begin\n"
    for bank in range(2):
        for port, other in [("a", "b"), ("b", "a")]:
            collision = (f"bank_{bank}_{other}_write_enable && "
                         f"bank_{bank}_{other}_address==bank_{bank}_{port}_address")
            text += (f"if(!bank_{bank}_{port}_write_enable)begin "
                     f"audit_taint_{bank}_{port}<={collision};"
                     f"if({collision})audit_collisions=audit_collisions+1;end\n")
    return text + "end\n" + vendor_body
