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
    run_capped(source, 20_000_000)
}
fn run_capped(source: &str, maximum_steps: usize) -> Vec<Vec<u16>> {
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
    let outcome = cpu.run(maximum_steps).unwrap();
    assert!(
        matches!(outcome, RunOutcome::Halted { signal: 0x600d, .. }),
        "{outcome:?}"
    );
    let rows = std::mem::take(&mut s.borrow_mut().rows);
    rows
}
fn fixed(row: &[u16], at: usize) -> f64 {
    let bits = u32::from(row[at]) | (u32::from(row[at + 1]) << 16);
    f64::from(bits as i32) / 65536.0
}
#[test]
fn radial_vectors_match_reference_across_travel_and_calibration() {
    let source = r#"mod stick;
fn emit(c:Array<StickPrepared>,x:u16,y:u16){let s=stick_read(c,x,y,true);
dev_send(7,0,x);dev_send(7,0,y);
dev_send(7,0,s.cal_x.lo_bits());dev_send(7,0,s.cal_x.hi_bits());
dev_send(7,0,s.cal_y.lo_bits());dev_send(7,0,s.cal_y.hi_bits());
dev_send(7,0,s.x.lo_bits());dev_send(7,0,s.x.hi_bits());
dev_send(7,0,s.y.lo_bits());dev_send(7,0,s.y.hi_bits());
dev_send(7,0,s.dir_x.lo_bits());dev_send(7,0,s.dir_x.hi_bits());
dev_send(7,0,s.dir_y.lo_bits());dev_send(7,0,s.dir_y.hi_bits());
dev_send(7,0,s.magnitude.lo_bits());dev_send(7,0,s.magnitude.hi_bits());
dev_send(7,0,s.valid);dev_send(7,1,0);}
fn main(){let mut c=stick_default();let mut profile:u16=0;while profile<3{
if profile==1{c.min_x=20;c.center_x=120;c.max_x=230;c.min_y=10;c.center_y=140;c.max_y=240;c.inner=0;c.outer=256;}
if profile==2{c=stick_default();c.inner=24;c.outer=200;}
let mut p=stick_prepare(view_of(&mut c));
let mut n:u16=0;while n<256{emit(view_of(&mut p),n,c.center_y);emit(view_of(&mut p),c.center_x,n);n+=1;}
let mut x:u16=0;while x<16{let mut y:u16=0;while y<16{emit(view_of(&mut p),x*17,y*17);y+=1;}x+=1;}
profile+=1;}halt(0x600d);}"#;
    let rows = run(source);
    assert_eq!(rows.len(), 2304);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), 17);
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
        let travel = x.abs().max(y.abs());
        let magnitude = ((travel - inner) / (outer - inner)).clamp(0., 1.);
        let (dx, dy) = if travel <= inner {
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
            let got = fixed(row, 2 + (at - 2) * 2);
            assert!(
                (got - expected).abs() <= 2. / 256.,
                "profile{profile} raw{:?} field{at}: got{got}, expected{expected}",
                &row[..2]
            );
        }
        assert_eq!(row[16], 1);
        assert!((0.0..=1.0).contains(&fixed(row, 14)));
        let norm = fixed(row, 6).hypot(fixed(row, 8));
        assert!(norm <= 257. / 256.);
    }
}
#[test]
fn invalid_disconnected_center_and_cardinal_hysteresis() {
    run(
        r#"mod stick;fn sample(c:Array<StickConfig>,x:u16,y:u16,on:bool)->StickSample{let mut p=stick_prepare(c);stick_read(view_of(&mut p),x,y,on)}
fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){let z=fix32::zero();let one=fix32::from_int(1);
let enter=fix32::from_words(16384,0);let release=fix32::from_words(8192,0);
let mut c=stick_default();let mut s=sample(view_of(&mut c),128,128,true);
check(s.valid==1&&s.x==z&&s.y==z&&s.dir_x==z&&s.dir_y==z&&s.magnitude==z);
s=sample(view_of(&mut c),255,0,false);check(s.valid==0&&s.x==z&&s.dir_x==z);
s=sample(view_of(&mut c),256,0,true);check(s.valid==0);
check(!stick_set_center(view_of(&mut c),0,128));check(c.center_x==128);
check(stick_set_center(view_of(&mut c),125,132));s=sample(view_of(&mut c),125,132,true);check(s.valid==1&&s.cal_x==z&&s.cal_y==z&&s.magnitude==z);
c.outer=c.inner;s=sample(view_of(&mut c),255,0,true);check(s.valid==0);
c=stick_default();c.center_y=255;check(!stick_config_valid(view_of(&mut c)));
c=stick_default();s=sample(view_of(&mut c),255,128,true);check(s.x>=fix32::from_words(65280,0)&&s.y==z&&s.dir_x>=fix32::from_words(65280,0)&&s.magnitude==one);
s.magnitude=fix32::from_words(16128,0);check(stick_cardinal(view_of(&mut s),0,enter,release)==0);
s.magnitude=enter;check(stick_cardinal(view_of(&mut s),0,enter,release)==1);
s.dir_x=-one;s.magnitude=fix32::from_words(12800,0);check(stick_cardinal(view_of(&mut s),1,enter,release)==1);
s.magnitude=enter;check(stick_cardinal(view_of(&mut s),1,enter,release)==3);
s.magnitude=release;check(stick_cardinal(view_of(&mut s),3,enter,release)==0);
s.magnitude=one;check(stick_cardinal(view_of(&mut s),0,release,release)==0);
check(stick_cardinal(view_of(&mut s),0,enter,-release)==0);
check(stick_cardinal(view_of(&mut s),0,z,z)==0);
check(stick_cardinal(view_of(&mut s),0,fix32::from_int(2),release)==0);
s.dir_x=z;s.dir_y=one;check(stick_cardinal(view_of(&mut s),0,enter,release)==2);
s.dir_y=-one;check(stick_cardinal(view_of(&mut s),0,enter,release)==4);
s.valid=0;check(stick_cardinal(view_of(&mut s),1,enter,release)==0);halt(0x600d);}"#,
    );
}

#[test]
fn square_ray_fraction_is_preserved_before_deadzone() {
    let rows = run(r#"mod stick;
fn main(){let mut c=stick_default();c.center_x=127;c.center_y=127;c.max_x=254;c.max_y=254;c.inner=0;
let mut p=stick_prepare(view_of(&mut c));
let mut n:u16=1;while n<=4{let mut d=n*32;if n==4{d=127;}
let a=stick_read(view_of(&mut p),127+d,127,true);
let b=stick_read(view_of(&mut p),127+d,127+d,true);
dev_send(7,0,a.magnitude.lo_bits());dev_send(7,0,a.magnitude.hi_bits());dev_send(7,0,b.magnitude.lo_bits());dev_send(7,0,b.magnitude.hi_bits());dev_send(7,0,b.x.lo_bits());dev_send(7,0,b.x.hi_bits());dev_send(7,0,b.y.lo_bits());dev_send(7,0,b.y.hi_bits());dev_send(7,1,0);n+=1;}halt(0x600d);}"#);
    assert_eq!(rows.len(), 4);
    for (i, row) in rows.iter().enumerate() {
        let distance = if i == 3 { 127.0 } else { (i + 1) as f64 * 32.0 };
        let strength = distance / 127.0;
        assert!((fixed(row, 0) - strength).abs() <= 1.0 / 256.0);
        assert!((fixed(row, 0) - fixed(row, 2)).abs() <= 1.0 / 256.0);
        assert!((fixed(row, 4) - strength / 2.0_f64.sqrt()).abs() <= 1.0 / 256.0);
        assert_eq!(fixed(row, 4), fixed(row, 6));
    }
    assert!(
        fixed(&rows[2], 2) < 200.0 / 256.0,
        "three-quarter diagonal must not saturate"
    );
}

#[test]
fn sample_retains_fractional_fpu_values_for_later_game_math() {
    let rows = run(r#"mod stick;
fn main(){let mut c=stick_default();c.inner=0;
let mut p=stick_prepare(view_of(&mut c));let s=stick_read(view_of(&mut p),160,176,true);
dev_send(7,0,s.cal_x.lo_bits());dev_send(7,0,s.cal_x.hi_bits());
dev_send(7,0,s.x.lo_bits());dev_send(7,0,s.x.hi_bits());
dev_send(7,0,s.magnitude.lo_bits());dev_send(7,0,s.magnitude.hi_bits());
let later=s.x*fix32::from_int(100);
dev_send(7,0,later.lo_bits());dev_send(7,0,later.hi_bits());dev_send(7,1,0);halt(0x600d);}"#);
    let row = &rows[0];
    assert_eq!(row.len(), 8);
    assert_ne!(row[0] & 255, 0, "calibration was quantized to Q8");
    assert_ne!(row[2] & 255, 0, "motion was quantized to Q8");
    assert_ne!(row[4] & 255, 0, "strength was quantized to Q8");
    assert!((fixed(row, 6) - fixed(row, 2) * 100.0).abs() < 1.0 / 65536.0);
}

#[test]
fn prepared_configuration_is_a_snapshot_until_explicit_rebuild() {
    run(r#"mod stick;fn check(ok:bool){if !ok{halt(0xbad);}}
fn main(){let mut c=stick_default();let mut p=stick_prepare(view_of(&mut c));
let before=stick_read(view_of(&mut p),139,128,true);check(before.magnitude==fix32::zero());
c.inner=0;let same=stick_read(view_of(&mut p),139,128,true);check(same.magnitude==fix32::zero());
p=stick_prepare(view_of(&mut c));let changed=stick_read(view_of(&mut p),139,128,true);check(changed.magnitude>fix32::zero());
check(stick_set_center(view_of(&mut c),139,128));p=stick_prepare(view_of(&mut c));
let centered=stick_read(view_of(&mut p),139,128,true);check(centered.cal_x==fix32::zero()&&centered.magnitude==fix32::zero());
c.outer=c.inner;p=stick_prepare(view_of(&mut c));check(p.valid==0);
let invalid=stick_read(view_of(&mut p),255,0,true);check(invalid.valid==0&&invalid.x==fix32::zero());halt(0x600d);}"#);
}

#[test]
fn unrefined_vector_rsqrt_covers_every_default_axis_pair_without_a_deadzone() {
    let rows = run_capped(
        r#"mod stick;
fn main(){let mut c=stick_default();c.inner=0;let mut p=stick_prepare(view_of(&mut c));
let mut x:u16=0;while x<256{let mut y:u16=0;while y<256{
let s=stick_read(view_of(&mut p),x,y,true);
dev_send(7,0,s.dir_x.lo_bits());dev_send(7,0,s.dir_x.hi_bits());
dev_send(7,0,s.dir_y.lo_bits());dev_send(7,0,s.dir_y.hi_bits());
dev_send(7,0,s.x.lo_bits());dev_send(7,0,s.x.hi_bits());
dev_send(7,0,s.y.lo_bits());dev_send(7,0,s.y.hi_bits());
dev_send(7,0,s.magnitude.lo_bits());dev_send(7,0,s.magnitude.hi_bits());dev_send(7,1,0);
y+=1;}x+=1;}halt(0x600d);}"#,
        100_000_000,
    );
    assert_eq!(rows.len(), 65_536);
    for (i, row) in rows.iter().enumerate() {
        let axis = |raw: usize| {
            let d = raw as f64 - 128.0;
            d / if d < 0.0 { 128.0 } else { 127.0 }
        };
        let x = axis(i / 256);
        let y = axis(i % 256);
        let radius = x.hypot(y);
        let strength = x.abs().max(y.abs());
        let (dx, dy) = if radius == 0.0 {
            (0.0, 0.0)
        } else {
            (x / radius, y / radius)
        };
        for (at, expected) in [
            (0, dx),
            (2, dy),
            (4, dx * strength),
            (6, dy * strength),
            (8, strength),
        ] {
            assert!(
                (fixed(row, at) - expected).abs() <= 1.0 / 128.0,
                "raw({}, {}) field{at}: {} vs {expected}",
                i / 256,
                i % 256,
                fixed(row, at)
            );
        }
        assert!((0.0..=1.0).contains(&fixed(row, 8)));
        assert!(fixed(row, 4).hypot(fixed(row, 6)) <= 257.0 / 256.0);
    }
}
