//! Query ordering, output transactions and page ownership through the actual RCC kernel.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::path::Path;

#[test]
fn terrain_queries_preserve_order_errors_and_mapping() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = root.join("../../../target/terrain-unit");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let ir = compile_program_named_checked(
        "terrain-branches",
        SOURCE,
        &options,
        &RustcCheck::new(output),
        &mut |name| {
            let name = name.rsplit('/').next().unwrap();
            if name == "trig_lookup" {
                return Ok("use crate::dsl_rt::*;pub fn index(table:Array<i16>,at:u16)->fix32{halt(0x7cfe);}".to_owned());
            }
            let path =
                if ["terrain","body","integer","sampling","ground","random","deformation","object_query","collision","driving","attitude","suspension","drive","vehicle"].contains(&name)
                {
                    root.join("applications/tankracer/rcc")
                        .join(format!("{name}.rs"))
                } else {
                    root.join("rcc").join(format!("{name}.rs"))
                };
            std::fs::read_to_string(path).map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let code = rcc_backend::try_compile(ir, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&code.words);
    let p = &mut cpu.physical_memory_mut()[0x100000..0x104000];
    p[0..16].copy_from_slice(&[0x5443, 1, 0, 300, 0, 0, 0, 0, 0x0f0f, 1, 16, 1, 17, 0, 0, 0]);
    p[16] = 32;
    p[17] = 256;
    p[32..48].copy_from_slice(&[
        192,
        64,
        160,
        3,
        0,
        32767,
        32767,
        77,
        11,
        (-12i16) as u16,
        13,
        (-14i16) as u16,
        1,
        0xe240,
        1,
        0,
    ]);
    p[64..128].fill(0);
    p[160..162].copy_from_slice(&[1, 255]); // [face1, face0, sentinel]
    p[192..208].copy_from_slice(&[
        0, 0, 500, 0, 0, 0, 0x10, 0, 1000, 208, 6, 232, 2, 248, 7000, 0,
    ]);
    p[208..226].copy_from_slice(&[
        0, 3, 0, 32767, 3, 0, 0, 3, 32767, 0, 99, 0, 32767, 99, 0, 0, 99, 32767,
    ]);
    p[232..244].copy_from_slice(&[0, 4096, 0, 0x0100, 2, 0x2100, 0, 4096, 0, 0x0403, 5, 0x2100]);
    p[248..254].copy_from_slice(&[1, 3, 1, 1, 3, 1]);
    p[256..272].copy_from_slice(&[
        192,
        280,
        3,
        4096,
        0,
        0,
        4096,
        (-1000i16) as u16,
        1000,
        (-1000i16) as u16,
        1000,
        0,
        0,
        0,
        0,
        0,
    ]);
    p[280..282].copy_from_slice(&[0x0100, 255]);
    for _ in 0..2_000_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(
                signal,
                0x600d,
                "pc={:#x}, auxiliary checkpoint={}",
                cpu.pc(),
                cpu.physical_memory_mut()[0x1000]
            );
            return;
        }
    }
    panic!("terrain unit exceeded instruction bound");
}

const SOURCE: &str = r#"
use crate::dsl_rt::*;
mod terrain; use terrain::*;
mod integer;
mod sampling;
mod ground;mod random;mod deformation;mod object_query;mod collision;
mod body;mod driving;mod attitude;mod suspension;mod drive;mod vehicle;mod trig_lookup;
mod static_assets; use static_assets::AssetBytes;
fn n(value:i16)->fix32 {integer::from_i16(value)}
fn sample_auxiliary(auxiliary:&mut sampling::Auxiliary,scope:&mut Terrain,matrix:Array<u16>,x:fix32,force:bool)->u16{
 let local:Buf<u16,6>=Buf::new([0,0,0,0,8,0]);
 fix32::store_words(local.as_array(),0,x);
 auxiliary.sample(scope,matrix,local.as_array(),force)
}
fn primary_branches(asset:AssetBytes){
 let directory:Buf<u16,64>=Buf::new([65535;64]);let mut dir=directory.as_array();dir[0u16]=0;
 let mut world=ground::World::begin(asset,fix32::from_words(0xabcd,0x1234),dir);
 assert!(world.seed==fix32::from_words(0x875d,0x487f));
 let mut i=0u16;while i<16{world.noise[i]=130;i+=1;}
 let tracks:Buf<u16,48>=Buf::new([4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0]);
 let config:Buf<u16,30>=Buf::new([0;30]);let mut c=config.as_array();c[2u16]=1;c[20u16]=3;
 sampling::put(c,11,n(100));sampling::put(c,12,n(2));sampling::put(c,13,n(5));sampling::put(c,14,n(1));
 let env:Buf<u16,20>=Buf::new([33,0,1,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]);let e=env.as_array();
 let records:Buf<deformation::Record,1>=Buf::new([deformation::record_zero()]);
 let mut surface=deformation::disabled(c,records.as_array());
 let mut state=ground::zero();i=0;
 while i<4{state.points[i].local_x=n(8);state.points[i].local_y=n(4795);state.points[i].local_z=n(8);state.points[i].travel=n(10);i+=1;}
 let mut data=unsafe{Ptr::from_addr(0xc000).as_u16_array()};data[39u16]=1;
 let seed=world.seed;
 assert!(ground::sample(&mut state,&mut world,tracks.as_array(),c,e,&mut surface)==0);
 assert!(world.seed==seed && state.load==n(1108) && state.material0==n(1));
 assert!(state.points[0u16].gap==n(0) && state.points[0u16].raw_height==n(4792));
 assert!(state.special==n(4) && state.flagged==n(4) && state.has_extra==n(1));
 let origin:Buf<u16,6>=Buf::new([0;6]);let edges:Buf<u16,8>=Buf::new([55536,65535,10000,0,55536,65535,10000,0]);let mesh:Buf<i16,3>=Buf::new([0,10,0]);
 surface.origin=origin.as_array();surface.basis=tracks.as_array();surface.bounds=edges.as_array();surface.edges=edges.as_array();surface.cap=fix32::from_words(0,1);surface.vertices=mesh.as_array();surface.vertex_count=1;surface.record_count=1;surface.attached=true;surface.enabled=true;
 assert!(ground::sample(&mut state,&mut world,tracks.as_array(),c,e,&mut surface)==0);
 assert!(state.points[0u16].gap==n(-80) && state.points[0u16].raw_height==n(4792) && state.points[0u16].kind==n(0));
 assert!(world.seed==seed && surface.dirty==1 && records[0u16].state==n(2));
 surface.enabled=false;
 let effect=state.effect.as_array();sampling::put(effect,0,n(1));sampling::put(effect,1,n(20));sampling::put(effect,2,n(1));sampling::put(effect,4,n(300));sampling::put(effect,5,n(1800));sampling::put(effect,6,n(77));sampling::put(e,1,n(4));
 assert!(ground::sample(&mut state,&mut world,tracks.as_array(),c,e,&mut surface)==0);
 assert!(world.seed==random::next(seed) && state.points[0u16].gap==n(-9));
 assert!(sampling::raw(effect,0)==n(-32) && sampling::raw(effect,2)==n(0));
 assert!(sampling::raw(effect,4)==n(200) && sampling::raw(effect,5)==n(900));
 assert!(sampling::raw(effect,6)==n(20) && state.clear_slot==n(1));
 let expired=world.seed;
 assert!(ground::sample(&mut state,&mut world,tracks.as_array(),c,e,&mut surface)==0);
 assert!(world.seed==expired && state.clear_slot==n(-1) && state.points[0u16].gap==n(0));
 data[39u16]=99;
 assert!(ground::sample(&mut state,&mut world,tracks.as_array(),c,e,&mut surface)==5 && state.load==n(0) && world.seed==expired);
 data[39u16]=1;
 let t=tracks.as_array();sampling::put(t,9,fix32::from_words(0,32768));sampling::put(t,21,fix32::from_words(0,32768));sampling::put(e,3,n(1));
 assert!(ground::sample(&mut state,&mut world,t,c,e,&mut surface)==0);
 assert!(state.points[0u16].kind==n(3) && state.points[0u16].raw_height==n(4792));
 assert!(state.points[0u16].surface==n(0) && state.points[0u16].gap==state.points[0u16].previous_gap);
 assert!(world.seed==expired && state.load==n(0));
 sampling::put(t,9,n(0));sampling::put(t,21,n(0));data[160u16]=65535;
 assert!(ground::sample(&mut state,&mut world,t,c,e,&mut surface)==0 && state.points[0u16].kind==n(4));
 assert!(state.points[0u16].ny==n(4096));
 data[160u16]=1;data[39u16]=77;world.terrain.finish();
}
fn rollback_branches(asset:AssetBytes){
 let directory:Buf<u16,64>=Buf::new([65535;64]);let mut dir=directory.as_array();dir[0u16]=0;
 let mut world=ground::World::begin(asset,fix32::from_words(0xabcd,0x1234),dir);
 let before=world.seed;let mut data=unsafe{Ptr::from_addr(0xc000).as_u16_array()};data[39u16]=1;
 let a:Buf<u16,58>=Buf::new([0;58]);let s:Buf<u16,88>=Buf::new([0;88]);let d:Buf<u16,32>=Buf::new([0;32]);let m:Buf<u16,32>=Buf::new([0;32]);let b:Buf<u16,8>=Buf::new([0;8]);
 let tracks:Buf<u16,48>=Buf::new([4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0]);
 let sc:Buf<u16,52>=Buf::new([0;52]);let dc:Buf<u16,520>=Buf::new([0;520]);let cfg:Buf<u16,36>=Buf::new([0;36]);let aux:Buf<u16,12>=Buf::new([0;12]);let f:Buf<u16,60>=Buf::new([0;60]);let events:Buf<u16,3>=Buf::new([0;3]);
 sampling::put(aux.as_array(),0,fix32::from_words(0,4));sampling::put(f.as_array(),0,n(33));sampling::put(f.as_array(),26,n(4));sampling::put(f.as_array(),27,n(1));
 sampling::put(dc.as_array(),47,n(1000));sampling::put(dc.as_array(),258,n(9000));sampling::put(dc.as_array(),259,n(1));
 sampling::put(a.as_array(),21,n(456));sampling::put(m.as_array(),4,n(777));sampling::put(s.as_array(),0,n(99));
 let mut car=driving::Car{attitude:a.as_array(),suspension:s.as_array(),drive:d.as_array(),motion:m.as_array(),boost:b.as_array(),tracks:tracks.as_array(),ground:ground::zero(),articulation:Buf::new([0;8]),lean:Buf::new([0;8]),hull:Buf::new([0;37]),ready:false,turret_z:n(0),gun:unsafe{tracks.as_ptr().as_i16_array()},gun_binding:tracks.as_array()};
 let mut i=0u16;while i<4 {car.ground.points[i].local_x=n(8);car.ground.points[i].local_y=n(4792);car.ground.points[i].local_z=n(8);car.ground.points[i].kind=n(55);i+=1;}
 sampling::put(car.ground.effect.as_array(),0,n(1));
 let config=driving::Config{spring:sc.as_array(),drive:dc.as_array(),attitude:cfg.as_array(),pose:cfg.as_array(),sampling:cfg.as_array(),auxiliary:aux.as_array(),environment:cfg.as_array(),body:cfg.as_array(),sine:unsafe{cfg.as_array().as_ptr().as_i16_array()},angle:unsafe{cfg.as_array().as_ptr().as_i16_array()}};
 let records:Buf<deformation::Record,1>=Buf::new([deformation::record_zero()]);
 let mut surface=deformation::disabled(cfg.as_array(),records.as_array());
 surface.enabled=true;surface.record_count=1;surface.attached=true;
 assert!(driving::tick(&mut car,&config,&mut world,f.as_array(),events.as_array(),&mut surface)==4);
 assert!(world.seed==before && sampling::raw(car.suspension,0)==n(99));
 assert!(sampling::raw(car.attitude,21)==n(456) && sampling::raw(car.motion,4)==n(777));
 assert!(car.ground.points[0u16].kind==n(55) && sampling::raw(car.ground.effect.as_array(),0)==n(1));
 assert!(surface.dirty==1 && records[0u16].state==n(2));
 assert!(sampling::raw(car.boost,0)==n(1000) && sampling::raw(car.boost,1)==n(8967) && sampling::raw(car.boost,3)==n(4967));
 data[39u16]=77;world.terrain.finish();
}
fn main(){
 let saved=read_dseg_page(3);
 let asset=AssetBytes{first_sector:0x4000,sector_count:256,byte_len_lo:32768,byte_len_hi:0};
 let mut scope=Terrain::begin(asset);
 let mut data=unsafe{Ptr::from_addr(0xc000).as_u16_array()};
 let mut hit=Hit{height:n(-55),node:n(-1),face:n(-2),nx:n(-12),ny:n(-13),nz:n(-14),
   surface:n(-3),special:n(-77),extra0:n(19),extra1:n(23),extra2:n(-29),extra3:n(31),
   flags:fix32::from_words(0x1234,0xabcd)};
 assert!(scope.query(n(8),n(8),&mut hit)==1);
 assert!(hit.height==n(4792) && hit.face==n(7024));
 assert!(hit.node==fix32::from_words(0xe240,1));
 assert!(hit.surface==n(77) && hit.special==n(1));
 assert!(hit.extra0==n(11) && hit.extra1==n(-12) && hit.extra2==n(13) && hit.extra3==n(-14));
 assert!(hit.flags==fix32::from_words(0x2030,0xabcd));
 primary_branches(asset);
 rollback_branches(asset);
 let mut checkpoint=unsafe{Ptr::from_addr(0x1000).as_u16_array()};
 checkpoint[0u16]=1;
 let transform:Buf<u16,24>=Buf::new([4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0,4096,0,0,0,0,0,0,0]);
 let matrix=transform.as_array();
 let mut auxiliary=sampling::Auxiliary{x:n(-1),y:n(-2),z:n(-3),kind:n(-11),surface:n(-12),cached:n(-13)};
 fix32::store_words(matrix,20,n(4716));
 assert!(sample_auxiliary(&mut auxiliary,&mut scope,matrix,n(8),false)==1);
 assert!(auxiliary.kind==n(0) && auxiliary.surface==n(77) && auxiliary.cached==n(1));
 checkpoint[0u16]=2;
 fix32::store_words(matrix,20,n(4717));
 assert!(sample_auxiliary(&mut auxiliary,&mut scope,matrix,n(8),false)==1 && auxiliary.kind==n(1));
 checkpoint[0u16]=3;
 fix32::store_words(matrix,20,n(4866));
 assert!(sample_auxiliary(&mut auxiliary,&mut scope,matrix,n(8),false)==1 && auxiliary.kind==n(1));
 checkpoint[0u16]=4;
 fix32::store_words(matrix,20,n(4867));
 assert!(sample_auxiliary(&mut auxiliary,&mut scope,matrix,n(8),true)==1);
 assert!(auxiliary.kind==n(2) && auxiliary.surface==n(1));
 checkpoint[0u16]=5;
 data[198u16]=0;
 hit.extra0=n(555);
 assert!(scope.query(n(8),n(8),&mut hit)==1 && hit.extra0==n(555));
 data[238u16]=4096; data[239u16]=0;
 assert!(scope.query(n(8),n(8),&mut hit)==2 && hit.height==n(4792));
 auxiliary.kind=n(-11);auxiliary.surface=n(-12);auxiliary.cached=n(-13);
 checkpoint[0u16]=6;
 assert!(sample_auxiliary(&mut auxiliary,&mut scope,matrix,n(8),true)==2);
 assert!(auxiliary.x==n(8) && auxiliary.y==n(4867) && auxiliary.z==n(8));
 assert!(auxiliary.kind==n(-11) && auxiliary.surface==n(-12) && auxiliary.cached==n(-13));
 checkpoint[0u16]=7;
 let found=scope.height(n(8),n(8)); assert!(found.status==1 && found.height==n(4000));
 let mut normal=Normal{x:n(99),y:n(99),z:n(99)};
 let found=scope.normal_height(n(8),n(8),&mut normal);
 assert!(found.status==1 && found.height==n(4000) && normal.x==n(4096) && normal.y==n(0));
 data[160u16]=0x0100; data[232u16]=32768; data[233u16]=65535;
 let found=scope.normal_height(n(0)+fix32::from_words(0,1),n(8),&mut normal);
 assert!(found.status==2 && normal.x==n(-32768) && normal.y==n(-1));
 data[232u16]=100; data[233u16]=4096;
 data[238u16]=65436; data[239u16]=4096;
 assert!(scope.fallback(n(8),n(4024),n(8),0,&mut normal)==1 && normal.x==n(100));
 data[198u16]=4;
 normal.x=n(99);
 assert!(scope.fallback(n(8),n(4024),n(8),1,&mut normal)==0 && normal.x==n(99));
 assert!(scope.fallback(n(8),n(4024),n(8),2,&mut normal)==1 && normal.x==n(100));
 data[36u16]=0x0d0d;
 assert!(scope.query(n(8000),n(0),&mut hit)==3 && hit.height==n(4792));
 assert!(scope.query(fix32::from_words(0,4),n(8),&mut hit)==4 && hit.height==n(4792));
 assert!(scope.query(fix32::from_words(0,32768),n(8),&mut hit)==0);
 assert!(hit.height==fix32::from_words(1,32768));
 checkpoint[0u16]=8;
 assert!(sample_auxiliary(&mut auxiliary,&mut scope,matrix,fix32::from_words(0,4),false)==4);
 assert!(auxiliary.x==fix32::from_words(0,4) && auxiliary.kind==n(-11));
 checkpoint[0u16]=9;
 fix32::store_words(matrix,18,fix32::from_words(0,32768));
 assert!(sample_auxiliary(&mut auxiliary,&mut scope,matrix,n(0),true)==0);
 assert!(auxiliary.kind==n(3) && auxiliary.surface==n(1) && auxiliary.cached==n(1));
 checkpoint[0u16]=10;
 // The product wraps before the arithmetic Q12 shift: MAX*2 -> -2 -> -1.
 data[36u16]=0;
 fix32::store_words(matrix,0,fix32::from_words(65535,32767));
 fix32::store_words(matrix,18,n(9));
 assert!(sample_auxiliary(&mut auxiliary,&mut scope,matrix,n(2),false)==1);
 assert!(auxiliary.x==n(8) && auxiliary.y==n(4867) && auxiliary.z==n(8));
 scope.bind(asset); assert!(scope.switches==1);
 scope.finish(); assert!(read_dseg_page(3)==saved);
 halt(0x600d);
}
"#;
