set here [file normalize [file dirname [info script]]]
cd $here
set_device -name GW2AR-18C -device_version C GW2AR-LV18QN88C8/I7
add_file -type verilog lighting.v
add_file -type verilog probe.v
add_file -type sdc lighting.sdc
set_option -synthesis_tool gowinsynthesis
set_option -top_module lighting_probe
set_option -output_base_name lighting
set_option -verilog_std sysv2017
set_option -gen_text_timing_rpt 1
set_option -print_all_synthesis_warning 1
run all
