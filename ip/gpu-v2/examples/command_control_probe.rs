use gpu_v2::command_processor::rtl;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).ok_or("expected output path")?;
    std::fs::write(output, rtl::verilog())?;
    Ok(())
}
