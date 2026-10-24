//! Compiled RCC against independent floating-point input processing.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, RunOutcome};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};
#[derive(Default)]
struct Records {
    words: Vec<u16>,
    rows: Vec<Vec<u16>>,
}
struct Sink(Rc<RefCell<Records>>);
impl Device for Sink {
    fn read(&mut self, _: &mut [u16], _: u8) -> u16 {
        0
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        let mut s = self.0.borrow_mut();
        if channel == 1 {
            let row = std::mem::take(&mut s.words);
            s.rows.push(row);
        } else {
            s.words.push(value);
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn run(source: &str) -> Vec<Vec<u16>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("stick reference", source, &options, &mut |n| {
            std::fs::read_to_string(root.join(format!("{n}.rs"))).map_err(|e| e.to_string())
        })
        .unwrap();
    let code = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    let s = Rc::new(RefCell::new(Records::default()));
    cpu.attach_device(7, Box::new(Sink(s.clone())));
    let outcome = cpu.run(20_000_000).unwrap();
    assert!(
        matches!(outcome, RunOutcome::Halted { signal: 0x600d, .. }),
        "{outcome:?}"
    );
    let rows = std::mem::take(&mut s.borrow_mut().rows);
    rows
}
#[test]
fn radial_vectors_match_reference_across_travel_and_calibration() {
    let source = r#"mod stick;
fn emit(c:Array<StickConfig>,x:u16,y:u16){let s=stick_read(c,x,y,true);
dev_send(7,0,x);dev_send(7,0,y);dev_send(7,0,s.cal_x as u16);dev_send(7,0,s.cal_y as u16);
dev_send(7,0,s.x as u16);dev_send(7,0,s.y as u16);dev_send(7,0,s.dir_x as u16);dev_send(7,0,s.dir_y as u16);dev_send(7,0,s.magnitude);dev_send(7,0,s.valid);dev_send(7,1,0);}
fn main(){let mut c=stick_default();let mut profile:u16=0;while profile<3{
if profile==1{c.min_x=20;c.center_x=120;c.max_x=230;c.min_y=10;c.center_y=140;c.max_y=240;c.inner=0;c.outer=256;}
if profile==2{c=stick_default();c.inner=24;c.outer=200;}
let mut n:u16=0;while n<256{emit(view_of(&mut c),n,c.center_y);emit(view_of(&mut c),c.center_x,n);n+=1;}
let mut x:u16=0;while x<16{let mut y:u16=0;while y<16{emit(view_of(&mut c),x*17,y*17);y+=1;}x+=1;}
profile+=1;}halt(0x600d);}"#;
    let rows = run(source);
    assert_eq!(rows.len(), 2304);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), 10);
        let profile = index / 768;
        let (min_x, cx, max_x, min_y, cy, max_y, inner, outer) = if profile == 1 {
            (20., 120., 230., 10., 140., 240., 0., 1.)
        } else {
            (
                0.,
                128.,
                255.,
                0.,
                128.,
                255.,
                24. / 256.,
                if profile == 2 { 200. / 256. } else { 1. },
            )
        };
        let axis = |v: u16, min: f64, center: f64, max: f64| {
            let d = f64::from(v) - center;
            (d / if d < 0.0 { center - min } else { max - center }).clamp(-1., 1.)
        };
        let x = axis(row[0], min_x, cx, max_x);
        let y = axis(row[1], min_y, cy, max_y);
        let radius = x.hypot(y);
        let magnitude = ((radius - inner) / (outer - inner)).clamp(0., 1.);
        let (dx, dy) = if radius <= inner {
            (0., 0.)
        } else {
            (x / radius, y / radius)
        };
        for (at, expected) in [
            (2, x),
            (3, y),
            (4, dx * magnitude),
            (5, dy * magnitude),
            (6, dx),
            (7, dy),
            (8, magnitude),
        ] {
            let got = f64::from(row[at] as i16);
            let expected = (expected * 256.).round();
            assert!(
                (got - expected).abs() <= 2.,
                "profile{profile} raw{:?} field{at}: got{got}, expected{expected}",
                &row[..2]
            );
        }
        assert_eq!(row[9], 1);
        assert!(row[8] <= 256);
        let norm = (f64::from(row[4] as i16)).hypot(f64::from(row[5] as i16));
        assert!(norm <= 257.);
    }
}
#[test]
fn invalid_disconnected_center_and_cardinal_hysteresis() {
    run(r#"mod stick;fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){let mut c=stick_default();let mut s=stick_read(view_of(&mut c),128,128,true);
check(s.valid==1&&s.x==0&&s.y==0&&s.dir_x==0&&s.dir_y==0&&s.magnitude==0);
s=stick_read(view_of(&mut c),255,0,false);check(s.valid==0&&s.x==0&&s.dir_x==0);
s=stick_read(view_of(&mut c),256,0,true);check(s.valid==0);
check(!stick_set_center(view_of(&mut c),0,128));check(c.center_x==128);
check(stick_set_center(view_of(&mut c),125,132));s=stick_read(view_of(&mut c),125,132,true);check(s.valid==1&&s.cal_x==0&&s.cal_y==0&&s.magnitude==0);
c.outer=c.inner;s=stick_read(view_of(&mut c),255,0,true);check(s.valid==0);
c=stick_default();c.center_y=255;check(!stick_config_valid(view_of(&mut c)));
c=stick_default();s=stick_read(view_of(&mut c),255,128,true);check(s.x>=255&&s.y==0&&s.dir_x>=255&&s.magnitude>=255);
s.magnitude=63;check(stick_cardinal(view_of(&mut s),0,64,32)==0);
s.magnitude=64;check(stick_cardinal(view_of(&mut s),0,64,32)==1);
s.dir_x=-256;s.magnitude=50;check(stick_cardinal(view_of(&mut s),1,64,32)==1);
s.magnitude=64;check(stick_cardinal(view_of(&mut s),1,64,32)==3);
s.magnitude=32;check(stick_cardinal(view_of(&mut s),3,64,32)==0);
s.magnitude=100;check(stick_cardinal(view_of(&mut s),0,32,32)==0);
s.dir_x=0;s.dir_y=256;check(stick_cardinal(view_of(&mut s),0,64,32)==2);
s.dir_y=-256;check(stick_cardinal(view_of(&mut s),0,64,32)==4);
s.valid=0;check(stick_cardinal(view_of(&mut s),1,64,32)==0);halt(0x600d);}"#);
}
