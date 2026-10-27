//! Bounded component checks for the CPU overlay and static demo assets.
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, RunOutcome};

fn compile(source: &str) -> Vec<u16> {
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("GPU demo overlay", source, &options, &mut |name| {
            match name {
                "controls" => Ok(include_str!("rcc/controls.rs").to_owned()),
                "overlay" => Ok(include_str!("rcc/overlay.rs").to_owned()),
                "frame_stats" => Ok(include_str!("../../rcc/frame_stats.rs").to_owned()),
                _ => Err(format!("unexpected module {name}")),
            }
        })
        .expect("RCC frontend");
    rcc_backend::try_compile(parsed, &options, "main")
        .expect("CPU V3 backend")
        .words
}

fn run(source: &str) -> CpuV3Sim {
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&compile(source));
    let result = cpu.run(5_000_000).expect("bounded CPU execution");
    assert!(matches!(result, RunOutcome::Halted { signal: 0x600d, .. }));
    cpu
}

fn read(cpu: &CpuV3Sim, address: u32) -> u16 {
    cpu.physical_memory(PhysicalWordAddress::new(address))
}

fn validate_overlay() {
    let cpu = run(r#"
mod overlay;
static STATE:Buf<u16,4>=Buf::new([0;4]);
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){
 let cells=unsafe{Ptr::from_addr(0x5000).as_u16_array()};
 let mut out=unsafe{Ptr::from_addr(0x6000).as_u16_array()};
 unsafe{Ptr::from_addr(0x4fff).write(0,0x1234);Ptr::from_addr(0x5177).write(0,0x5678);}
 demo_overlay_init(cells,STATE.as_array());
 let mut i:u16=0;while i<375{out[i]=cells[i];i+=1;}
 demo_overlay_begin(cells,STATE.as_array(),65530,true);
 i=0;while i<375{out[375+i]=cells[i];i+=1;}
 demo_overlay_begin(cells,STATE.as_array(),11,true);
 i=0;while i<375{out[750+i]=cells[i];i+=1;}
 demo_overlay_begin(cells,STATE.as_array(),100,false);
 i=0;while i<375{out[1125+i]=cells[i];i+=1;}
 demo_overlay_begin(cells,STATE.as_array(),109,true);
 i=0;while i<375{out[1500+i]=cells[i];i+=1;}
 demo_overlay_begin(cells,STATE.as_array(),109,true);
 i=0;while i<375{out[1875+i]=cells[i];i+=1;}
 demo_overlay_begin(cells,STATE.as_array(),119,true);
 i=0;while i<375{out[2250+i]=cells[i];i+=1;}
 // Every cell, including row boundaries and the last cell, retains its neighbor.
 i=0;while i<750{
   let before=cells[i>>1];check(demo_cell_set(cells,i,i&255));
   if i&1==0{check((cells[i>>1]&65280)==(before&65280));}
   else{check((cells[i>>1]&255)==(before&255));}i+=1;
 }
 check(!demo_cell_set(cells,750,0));check(!demo_cell_set(cells,65535,0));
 check(!demo_cell_set(cells,1,256));
 halt(0x600d);
}
"#);
    for (frame, digits) in ["", "", "17", "", "9", "0", "10"].into_iter().enumerate() {
        let mut wanted = [255u8; 750];
        wanted[52..52 + digits.len()].copy_from_slice(digits.as_bytes());
        for (i, pair) in wanted.as_chunks::<2>().0.iter().enumerate() {
            assert_eq!(
                read(&cpu, 0x6000 + (frame * 375 + i) as u32),
                u16::from_le_bytes([pair[0], pair[1]]),
                "frame {frame}, word {i}"
            );
        }
    }
    for i in 0..375 {
        assert_eq!(
            read(&cpu, 0x5000 + i),
            ((2 * i) as u8 as u16) | (((2 * i + 1) as u8 as u16) << 8)
        );
    }
    assert_eq!(read(&cpu, 0x4fff), 0x1234);
    assert_eq!(read(&cpu, 0x5177), 0x5678);
}

fn validate_font() {
    use gpu_v2::system::vliw_command::ui_char;
    let mut seen = [false; 255];
    for line in include_str!("assets/font-8x16.txt").lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 17);
        let glyph: usize = fields[0].parse().unwrap();
        assert!(glyph < 255 && !seen[glyph]);
        seen[glyph] = true;
        let rows: Vec<_> = fields[1..]
            .iter()
            .map(|row| u8::from_str_radix(row, 16).unwrap())
            .collect();
        let pixels = std::array::from_fn(|y| {
            std::array::from_fn(|x| if rows[y] & (128 >> x) != 0 { 65535 } else { 0 })
        });
        let native = ui_char(&pixels);
        for (y, row) in pixels.iter().enumerate() {
            for (x, &pixel) in row.iter().enumerate() {
                let at = (y * 8 + (x ^ ((y & 1) * 2))) * 2;
                assert_eq!(u16::from_le_bytes([native[at], native[at + 1]]), pixel);
            }
        }
    }
    for glyph in b"FRAME 0123456789-MS." {
        assert!(seen[usize::from(*glyph)]);
    }
}

fn main() {
    validate_overlay();
    validate_font();
    println!("PASS demo components: packed cells, completed-frame text, native font");
}

#[cfg(test)]
mod tests {
    #[test]
    fn compiled_cpu_updates_packed_cells_and_completed_frame_text() {
        super::validate_overlay();
    }
    #[test]
    fn source_font_matches_native_ui_encoding() {
        super::validate_font();
    }
}

#[test]
fn button_toggle_debounces_bounce_hold_release_and_timer_wrap() {
    run(r#"
use crate::dsl_rt::*;
mod controls; use controls::*;
fn check(v: bool) { if !v { halt(1); } }
fn main() {
 let mut s: Buf<u16,4> = Buf::new([0;4]);
 demo_buttons_init(s.as_array(),0,65520);
 check(demo_buttons_update(s.as_array(),1,65525));
 check(demo_buttons_update(s.as_array(),0,65530));
 check(demo_buttons_update(s.as_array(),1,65535));
 check(demo_buttons_update(s.as_array(),1,18));
 check(!demo_buttons_update(s.as_array(),1,19));
 check(!demo_buttons_update(s.as_array(),1,100));
 check(!demo_buttons_update(s.as_array(),0,101));
 check(!demo_buttons_update(s.as_array(),0,121));
 check(!demo_buttons_update(s.as_array(),1,130));
 check(demo_buttons_update(s.as_array(),1,150));
 halt(0x600d);
}
"#);
}
