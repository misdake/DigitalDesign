//! Bounded compiled-RCC input tests with independent coherent/torn Device5 peers.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, RunOutcome, StepOutcome};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};

struct Wire {
    valid: bool,
    changed: bool,
    id: u16,
    buttons: u16,
    right: u16,
    left: u16,
    tear: Option<u8>,
    attempts: usize,
    payload_reads: usize,
    conversions: u16,
}
impl Default for Wire {
    fn default() -> Self {
        Self {
            valid: true,
            changed: true,
            id: 115,
            buttons: 0,
            right: 190 | (85 << 8),
            left: 210 | (100 << 8),
            tear: None,
            attempts: 0,
            payload_reads: 0,
            conversions: 0,
        }
    }
}
struct Input(Rc<RefCell<Wire>>);
impl Device for Input {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.0.borrow_mut();
        if channel == 8 {
            return u16::from(s.valid) | (u16::from(s.changed) << 1);
        }
        s.payload_reads += 1;
        if s.tear == Some(channel) || s.tear == Some(channel + 16) {
            if s.tear == Some(channel + 16) {
                s.valid = false;
            }
            s.tear = None;
            s.changed = true;
            s.buttons ^= 0x8000;
        }
        match channel {
            0 => s.left & 255,
            1 => s.left >> 8,
            2 => s.id,
            3 => 0x5a,
            4 => (s.buttons ^ 65535) & 255,
            5 => (s.buttons ^ 65535) >> 8,
            6 => s.right & 255,
            7 => s.right >> 8,
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, _: u16) {
        assert_eq!(channel, 8, "software must not drive the serial transport");
        let mut s = self.0.borrow_mut();
        s.changed = false;
        s.attempts += 1;
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
struct Control(Rc<RefCell<Wire>>);
impl Device for Control {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let s = self.0.borrow();
        match channel {
            0 => s.attempts as u16,
            1 => s.payload_reads as u16,
            2 => s.conversions,
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        let mut s = self.0.borrow_mut();
        match channel {
            0 => s.buttons = value,
            1 => s.id = value,
            2 => s.right = value,
            3 => s.left = value,
            4 => s.valid = value != 0,
            5 => s.tear = Some(value as u8),
            _ => panic!("unknown peer control"),
        }
        s.changed = true;
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn run(source: &str, maximum_steps: usize) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("gamepad peer", source, &options, &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        })
        .unwrap_or_else(|e| panic!("{e}"));
    if let Some(cache) = parsed.debug.types.iter().find(|t| t.name == "GamepadStick") {
        let frame = parsed
            .debug
            .types
            .iter()
            .find(|t| t.name == "GamepadFrame")
            .unwrap();
        let sample = parsed
            .debug
            .types
            .iter()
            .find(|t| t.name == "StickSample")
            .unwrap();
        // The unsafe native-word installation must stop before cache keys and
        // preserve the source/destination field spans when layouts evolve.
        assert_eq!(cache.size, 32);
        assert_eq!(frame.size, 13);
        let offset = |t: &rcc::DebugType, name: &str| {
            t.fields.iter().find(|f| f.name == name).unwrap().offset
        };
        assert_eq!(offset(cache, "valid") - offset(cache, "x") + 1, 11);
        assert_eq!(offset(sample, "valid") - offset(sample, "x") + 1, 11);
        assert_eq!(offset(cache, "raw_pair"), offset(cache, "valid") + 1);
    }
    let code = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let wire = Rc::new(RefCell::new(Wire::default()));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    cpu.attach_device(5, Box::new(Input(wire.clone())));
    cpu.attach_device(7, Box::new(Control(wire.clone())));
    let entry = code
        .debug
        .functions
        .iter()
        .find(|f| f.name == "stick_read")
        .map(|f| f.addr.0 as u16);
    let mut result = RunOutcome::StepLimit {
        steps: maximum_steps,
    };
    for steps in 1..=maximum_steps {
        if entry == Some(cpu.pc()) {
            wire.borrow_mut().conversions += 1;
        }
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            result = RunOutcome::Halted { steps, signal };
            break;
        }
    }
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
}

#[test]
fn repeated_capture_only_converts_the_last_consumed_raw_and_invalidates_lazily() {
    run(
        r#"mod gamepad;
fn check(ok:bool,code:u16){if !ok{halt(code);}}
fn count()->u16{dev_recv(7,2)}
fn main(){gamepad_init();check(gamepad_capture(),0xb301);let before=count();let mut i:u16=0;
while i<4{dev_send(7,0,1<<i);dev_send(7,3,(160+i)|(100<<8));dev_send(7,2,(80+i)|(180<<8));check(gamepad_capture()&&count()==before,0xb302);i+=1;}
let f=gamepad_take_frame();check(count()==before+2&&f.pressed==15&&f.released==7&&f.buttons==8,0xb303);
let direction=gamepad_direction(0);let strength=gamepad_strength(0);check(gamepad_stick_valid(0)&&count()==before+2,0xb304);
dev_send(7,3,200|(100<<8));check(gamepad_capture()&&count()==before+2,0xb305);let d=gamepad_direction(0);check(count()==before+3&&d.x()!=direction.x(),0xb306);
check(gamepad_stick_valid(0)&&gamepad_strength(0)>strength&&count()==before+3,0xb307);
dev_send(7,3,220|(90<<8));check(gamepad_capture()&&count()==before+3,0xb308);let high=gamepad_strength(0);check(count()==before+4&&high>strength,0xb309);
dev_send(7,3,180|(120<<8));check(gamepad_capture(),0xb30a);dev_send(7,3,220|(90<<8));check(gamepad_capture(),0xb30b);
let reused=gamepad_take_frame();check(count()==before+4,0xb30c);
let mut config=stick_default();config.inner=0;check(gamepad_configure(0,view_of(&mut config))&&count()==before+4,0xb30d);
let configured=gamepad_strength(0);check(count()==before+5&&configured>high,0xb30e);let same=gamepad_take_frame();check(count()==before+5,0xb30f);
dev_send(7,4,0);check(!gamepad_capture()&&count()==before+5,0xb310);check(!gamepad_stick_valid(0)&&count()==before+6,0xb317);let off=gamepad_direction(1);check(off.x()==fix32::zero()&&off.y()==fix32::zero(),0xb311);
check(gamepad_strength(0)==fix32::zero()&&!gamepad_stick_valid(0)&&count()==before+7,0xb312);
dev_send(7,1,65);dev_send(7,4,1);check(gamepad_capture()&&!gamepad_stick_valid(1)&&count()==before+7,0xb313);
dev_send(7,1,121);check(gamepad_capture()&&count()==before+7,0xb314);let reconnected=gamepad_strength(1);check(count()==before+8&&reconnected>fix32::zero(),0xb315);
let final_frame=gamepad_take_frame();check(count()==before+9&&final_frame.id==121&&f.buttons==8&&f.pressed==15,0xb316);halt(0x600d);}"#,
        300_000,
    );
}

#[test]
fn capture_accumulates_edges_and_rejects_torn_samples_until_explicit_consumption() {
    run(
        r#"mod dualshock2;
static S:Buf<u16,10>=Buf::new([0;10]);static RX:Buf<u16,9>=Buf::new([0;9]);
fn check(ok:bool,code:u16){if !ok{halt(code);}}
fn capture(){check(ds2_capture(S.as_array(),RX.as_array()),0xb001);}
fn main(){let s=S.as_array();ds2_init(s);capture();check(s[8u16]==0&&s[9u16]==0,0xb002);
dev_send(7,0,1);capture();check(s[8u16]==1&&s[9u16]==0,0xb003);
let reads=dev_recv(7,1);check(!ds2_capture(s,RX.as_array()),0xb004);check(dev_recv(7,1)==reads&&s[8u16]==1,0xb005);
dev_send(7,0,0);capture();dev_send(7,0,2);capture();check(s[2u16]==2&&s[8u16]==3&&s[9u16]==1,0xb006);
dev_send(7,5,5);check(!ds2_capture(s,RX.as_array()),0xb007);check(s[2u16]==2&&s[8u16]==3&&s[9u16]==1,0xb008);
capture();check(s[2u16]==0x8002&&s[8u16]==0x8003&&s[9u16]==1,0xb009);
ds2_consume_edges(s);check(s[8u16]==0&&s[9u16]==0&&s[2u16]==0x8002,0xb00a);
dev_send(7,0,0);capture();check(s[9u16]==0x8002,0xb00b);
dev_send(7,4,0);check(!ds2_capture(s,RX.as_array()),0xb00c);check(s[0u16]==0&&s[8u16]==0&&s[9u16]==0&&s[3u16]==128,0xb00d);
dev_send(7,4,1);dev_send(7,0,7);capture();check(s[2u16]==7&&s[8u16]==0&&s[9u16]==0,0xb00e);
dev_send(7,1,65);capture();check(s[3u16]==128&&s[4u16]==128&&s[5u16]==128&&s[6u16]==128,0xb00f);
dev_send(7,0,6);check(ds2_service(s,RX.as_array())&&s[9u16]==1,0xb010);
check(!ds2_service(s,RX.as_array())&&s[8u16]==0&&s[9u16]==0,0xb011);
dev_send(7,0,7);capture();dev_send(7,5,21);check(!ds2_capture(s,RX.as_array()),0xb012);
check(s[0u16]==0&&s[8u16]==0&&s[9u16]==0&&s[6u16]==128,0xb013);
dev_send(7,4,1);capture();check(s[8u16]==0&&s[9u16]==0,0xb014);halt(0x600d);}"#,
        100_000,
    );
}

#[test]
fn game_frames_consume_once_and_keep_motion_without_new_samples_or_configuration() {
    run(
        r#"mod gamepad;
fn check(ok:bool,code:u16){if !ok{halt(code);}}
fn main(){check(!gamepad_capture(),0xb101);let mut config=stick_default();check(!gamepad_configure(0,view_of(&mut config)),0xb102);
gamepad_init();check(gamepad_capture(),0xb103);let a=gamepad_take_frame();check(a.connected==1&&a.id==115&&a.left_x>fix32::zero(),0xb104);
dev_send(7,0,1);check(gamepad_capture(),0xb105);check(!gamepad_capture(),0xb106);
dev_send(7,0,0);check(gamepad_capture(),0xb107);let b=gamepad_take_frame();check(b.pressed==1&&b.released==1&&b.buttons==0,0xb108);
let reads=dev_recv(7,1);check(!gamepad_capture(),0xb109);let c=gamepad_take_frame();check(dev_recv(7,1)==reads&&c.pressed==0&&c.released==0&&c.left_x==a.left_x&&c.right_y==a.right_y,0xb10a);
let before=gamepad_strength(0);config.inner=0;check(gamepad_configure(0,view_of(&mut config)),0xb10b);
check(!gamepad_capture()&&gamepad_strength(0)>before,0xb10c);
config.center_x=210;config.center_y=100;check(gamepad_configure(0,view_of(&mut config)),0xb10d);let d=gamepad_take_frame();check(d.left_x==fix32::zero()&&d.left_y==fix32::zero(),0xb10e);
config.outer=config.inner;check(!gamepad_configure(0,view_of(&mut config))&&!gamepad_configure(2,view_of(&mut config)),0xb10f);
dev_send(7,1,65);check(gamepad_capture(),0xb110);let e=gamepad_take_frame();check(e.connected==1&&e.left_x==fix32::zero()&&e.right_y==fix32::zero()&&!gamepad_stick_valid(0),0xb111);
dev_send(7,4,0);check(!gamepad_capture(),0xb112);let f=gamepad_take_frame();check(f.connected==0&&f.buttons==0&&f.pressed==0&&f.left_x==fix32::zero(),0xb113);
dev_send(7,1,121);dev_send(7,0,0x8000);dev_send(7,4,1);check(gamepad_capture(),0xb114);let g=gamepad_take_frame();check(g.id==121&&g.buttons==0x8000&&g.pressed==0&&g.released==0&&gamepad_stick_valid(1),0xb115);
let z=gamepad_direction(2);check(z.x()==fix32::zero()&&z.y()==fix32::zero()&&gamepad_strength(2)==fix32::zero()&&!gamepad_stick_valid(2),0xb116);halt(0x600d);}"#,
        200_000,
    );
}

#[test]
fn cached_motion_direction_and_strength_match_the_original_kernel_bit_for_bit() {
    run(
        r#"mod gamepad;
fn check(ok:bool){if !ok{halt(0xb201);}}
fn compare(p:Array<StickPrepared>,x:u16,y:u16){
dev_send(7,3,x|(y<<8));dev_send(7,2,y|(x<<8));check(gamepad_capture());
let a=stick_read(p,x,y,true);let b=stick_read(p,y,x,true);let f=gamepad_take_frame();
let l=gamepad_direction(0);let r=gamepad_direction(1);
check(f.left_x==a.x&&f.left_y==a.y&&f.right_x==b.x&&f.right_y==b.y);
check(l.x()==a.dir_x&&l.y()==a.dir_y&&r.x()==b.dir_x&&r.y()==b.dir_y);
check(gamepad_strength(0)==a.magnitude&&gamepad_strength(1)==b.magnitude&&gamepad_stick_valid(0)&&gamepad_stick_valid(1));
check(!gamepad_capture());let held=gamepad_take_frame();check(held.left_x==f.left_x&&held.right_y==f.right_y);}
fn main(){gamepad_init();let mut c=stick_default();let mut profile:u16=0;while profile<3{
if profile==1{c.min_x=20;c.center_x=120;c.max_x=230;c.min_y=10;c.center_y=140;c.max_y=240;c.inner=0;c.outer=256;}
if profile==2{c=stick_default();c.inner=24;c.outer=200;}
check(gamepad_configure(0,view_of(&mut c))&&gamepad_configure(1,view_of(&mut c)));let mut p=stick_prepare(view_of(&mut c));
compare(view_of(&mut p),128,128);compare(view_of(&mut p),0,255);compare(view_of(&mut p),255,0);compare(view_of(&mut p),160,176);compare(view_of(&mut p),139,128);
let mut x:u16=0;while x<4{let mut y:u16=0;while y<4{compare(view_of(&mut p),x*85,y*85);y+=1;}x+=1;}profile+=1;}
halt(0x600d);}"#,
        2_000_000,
    );
}
