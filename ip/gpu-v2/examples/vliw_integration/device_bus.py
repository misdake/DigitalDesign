"""Testbench calls for the production two-SEND Device4 interface."""
TASKS = r"""
task send_gpu;
 input[3:0]channel;input[15:0]value;
 begin
  @(negedge clk);#1;
  device_index=4;device_channel=channel;device_write_data=value;device_write_enable=1;device_read_enable=0;
  @(posedge clk);#1;device_write_enable=0;
  device_channel=2;device_read_enable=1;#1;
 end
endtask
task submit_commands;
 input[31:0]base,bytes;
 begin
  if(busy||base%128||bytes==0||bytes%4||bytes>65536||base+((bytes+127)/128)*128>32'h800000)
   $fatal(1,"Driver submission contract");
  send_gpu(0,bytes/4-1);send_gpu(1,base/128);
  if(!busy||device_read_data[0]!==1'b1)$fatal(1,"SEND did not latch busy");
 end
endtask
"""

def bench(text):
    """Add protocol tasks to fixtures whose production ports are generated."""
    return text.replace('gpu_v2_vliw_command dut(.*);', TASKS+'\ngpu_v2_vliw_command dut(.*);')


def system_leaf(text, ports, reset_expression='reset'):
    """Bind the actual system leaf while keeping observations on its real core."""
    links=['.clk(clk)',f'.reset({reset_expression})']+[f'.{n}({n})' for n in
        ['device_index','device_channel','device_read_enable','device_write_enable','device_write_data','device_read_data']]
    for prefix in ['ro','fb_r','fb_w']:
        for field in ['request_valid','request_ready','response_valid','response_last','address']:
            links.append(f'.gpu_{prefix}_{field}({prefix}_{field})')
        links += [f'.gpu_{prefix}_line_count_minus_1({prefix}_count)',f'.gpu_{prefix}_error({prefix}_response_error)']
        if prefix=='fb_w':links += ['.gpu_fb_w_write_data(fb_w_data)','.gpu_fb_w_write_data_ready(fb_w_data_ready)']
        else:links += [f'.gpu_{prefix}_read_data({prefix}_data)',f".gpu_{prefix}_write_data_ready(1'b0)"]
    marker='gpu_v2_vliw_command dut(.*);'
    for port in ports:
        name=port['name']
        if port['direction']=='output' and name!='device_read_data' and not name.startswith(('ro_','fb_r_','fb_w_')):
            text=text.replace(marker,f'assign {name}=dut.{name};\n'+marker)
    text=text.replace('dut.','dut.core.')
    return text.replace(marker,'CpuV3Gpu dut('+','.join(links)+');')
