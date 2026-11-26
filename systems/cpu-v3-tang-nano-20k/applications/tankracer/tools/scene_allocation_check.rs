//! Shared transient allocation/release through real class tables and banked rows.
use super::*;
fn u16s(value: &Value) -> Vec<u16> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u16)
        .collect()
}
fn check_words(
    memory: &[u16],
    at: usize,
    want: &[u16],
    label: &str,
) -> Result<usize, Box<dyn Error>> {
    for (i, &value) in want.iter().enumerate() {
        if memory[at + i] != value {
            return Err(format!("{label} word {i}: {} != {value}", memory[at + i]).into());
        }
    }
    Ok(want.len())
}
pub fn run(output: &Path, fixture: &Path, label: &str) -> Result<Value, Box<dyn Error>> {
    let creation = label != "scene-allocation";
    let fire_mode = label == "scene-fire";
    let laps = label == "scene-laps";
    let ufo_tick = label == "scene-ufo-transfer";
    let vehicle_methods = label == "scene-vehicle-transfer" || ufo_tick;
    let obstacle_methods = label == "scene-obstacle-transfer";
    let ufos = label == "scene-ufos";
    let projectile_ufo = label == "scene-projectile-ufo";
    let has_ufos = ufos || projectile_ufo || ufo_tick;
    let mines = label == "scene-mines" || label == "scene-mine-tick";
    let mine_hit = label == "scene-mine-hit";
    let projectile_direct = label == "scene-projectile-direct";
    let projectile_area = label == "scene-projectile-area" || projectile_direct || projectile_ufo;
    let projectile_flight = label == "scene-projectile-flight" || projectile_area;
    let mine_membership =
        mines || mine_hit || has_ufos || vehicle_methods || obstacle_methods || fire_mode || laps;
    let explosion_tick = label == "scene-explosion-tick";
    let weapon_explosion = label == "scene-weapon-explosion" || label == "scene-mine-explosion";
    let vehicle_views = explosion_tick || weapon_explosion || mine_hit || projectile_area;
    let explosions = label == "scene-explosions" || vehicle_views;
    let lap_setup = if laps {
        r#"
 world.grid=spatial::Grid{page:119,origin:pointer(0xf2c0),shift_x:scene.header[14u16],shift_z:scene.header[15u16],items:pointer(0xd200),ranges:pointer(0xf200)};
 world.deformables=ordered::List{items:pointer(0x7000),count:pointer(0x6f90)[0u16],capacity:128};
 let mut attachments:Buf<scene_laps::Attachment,16>=Buf::new([scene_laps::Attachment{object:0,present:pointer(0),attached:pointer(0)};16]);
 let mut i=0u16;while i<pointer(0x6f88)[0u16]{attachments[i]=scene_laps::Attachment{object:pointer(0x6f40)[i],present:pointer(0x6f00+i*2),attached:pointer(0x6f01+i*2)};i+=1;}
 let mut lap_context:Buf<scene_laps::Context,1>=Buf::new([scene_laps::Context{objects:pointer(0x6100),count:pointer(0x6f8a)[0u16],backups:pointer(0x6d00),backup_count:pointer(0x6f89)[0u16],attachments:attachments.as_array(),attachment_count:pointer(0x6f88)[0u16],camera:pointer(0x7100),lap:0,flush:false}]);
"#
    } else {
        ""
    };
    let lap_call = if laps {
        r#"
 lap_context[0u16].lap=g(c,1).lo_bits() as i16;lap_context[0u16].flush=g(c,2)!=integer::from_i16(0);lap_context[0u16].camera=pointer(0x7100+g(c,3).lo_bits()*6);
 status=scene_laps::step(&scene,&mut store,&mut runtime,&mut world,&mut weapons,lap_context.as_array());
 let mut published=pointer(0x6f90);published[0u16]=world.deformables.count;
"#
    } else {
        "halt(0x7e86);"
    };
    let obstacle_setup = if obstacle_methods {
        r#"
 world.count=pointer(0x66fc)[0u16];
 world.grid=spatial::Grid{page:119,origin:pointer(0xf2c0),shift_x:scene.header[14u16],shift_z:scene.header[15u16],items:pointer(0xd200),ranges:pointer(0xf200)};
"#
    } else {
        ""
    };
    let obstacle_call = if obstacle_methods {
        r#"
 entity=g(c,5);let call=scene_obstacle_transfer::Call{entity,slot:g(c,1).lo_bits(),payload:g(c,2).lo_bits(),value:g(c,3),position:pointer(0x66e0),shape_present:pointer(0x66f0)[0u16]!=0,result:pointer(0x66f8)};
 status=scene_obstacle_transfer::dispatch(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&call);
"#
    } else {
        "halt(0x7e83);"
    };
    let vehicle_setup = if vehicle_methods {
        r#"
 world.grid=spatial::Grid{page:119,origin:pointer(0xf2c0),shift_x:scene.header[14u16],shift_z:scene.header[15u16],items:pointer(0xd200),ranges:pointer(0xf200)};
 let row=pointer(0x6800);let transfer_meta=pointer(0x66f0);
 let pose=vehicle_pose::State{physical_position:row,reference:at(row,6),base_position:at(row,24),body_basis:at(row,30),body_position:at(row,48),tracks:at(row,54),turret:at(row,102),gun:at(row,126),attachments:at(row,150),bottom_position:at(row,198),recoil:at(row,204)};
 let car=vehicle_transfer::Car{pose,checkpoint:at(row,208),passed:at(row,212),laps:at(row,214),controls:at(row,216),velocity:at(row,222),acceleration:at(row,228),angular:at(row,232),track_slip:at(row,238),temporary_timer:at(row,240),config:pointer(0x6a00),travel:pointer(0x6a70),environment:pointer(0x6a40)};
 let transfer_cars:Buf<scene_vehicle_transfer::Car,1>=Buf::new([scene_vehicle_transfer::Car{present:transfer_meta[4u16]!=0,transfer:car,flags:at(row,244)}]);
 let routes=vehicle_transfer::Routes{order:ordered::List{items:pointer(0x6b00),count:transfer_meta[0u16],capacity:128},points:pointer(0x7000),count:transfer_meta[1u16],occupied:ordered::List{items:pointer(0x6c00),count:transfer_meta[2u16],capacity:128}};
 let mut routes_owner:Buf<vehicle_transfer::Routes,1>=Buf::new([routes]);
 let vehicle_context=scene_vehicle_transfer::View{cars:transfer_cars.as_array(),count:1,present:transfer_meta[3u16]!=0,routes:routes_owner.as_array(),sine:launch.context.tables.sine,angle:launch.context.tables.angle,result:pointer(0x66f8)};
"#
    } else {
        ""
    };
    let vehicle_setup = if ufo_tick {
        vehicle_setup.replace("let transfer_cars:Buf<scene_vehicle_transfer::Car,1>=Buf::new([scene_vehicle_transfer::Car{present:transfer_meta[4u16]!=0,transfer:car,flags:at(row,244)}]);", "let transfer_cars:Buf<scene_vehicle_transfer::Car,3>=Buf::new([scene_vehicle_transfer::Car{present:transfer_meta[4u16]!=0,transfer:car,flags:at(row,244)},transfer_car(0x6900,1),transfer_car(0x6e00,2)]);")
        .replace("let vehicle_context=", "let mut vehicle_context=")
        .replace("count:1,present:transfer_meta", "count:3,present:transfer_meta")
    } else {
        vehicle_setup.to_owned()
    };
    let actor_setup = if ufo_tick {
        r#"
 world.count=pointer(0x66fc)[0u16];
 let visible:Buf<ordered::List,1>=Buf::new([ordered::List{items:pointer(0x6d00),count:pointer(0x6340)[0u16],capacity:128}]);
 let mut ufo_actor=scene_ufo_transfer::Tick{call:scene_creation::Call{entity:integer::from_i16(0),slot:12},dt:integer::from_i16(0),resources_present:pointer(0x6341)[0u16]!=0,height:g(pointer(0x6342),0),vehicles:view_of(&mut vehicle_context),car_info:pointer(0x6100),racers:pointer(0x6120),visible:visible.as_array(),shapes:pointer(0x6140),overlay:pointer(0x6520)};
"#
    } else {
        ""
    };
    let actor_call = if ufo_tick {
        r#"
 entity=g(c,3);ufo_actor.call.entity=entity;ufo_actor.call.slot=g(c,2).lo_bits();ufo_actor.dt=g(c,1);
 status=scene_ufo_transfer::dispatch(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&mut ufo_actor);
 let mut published=pointer(0x66f2);published[0u16]=vehicle_context.routes[0u16].occupied.count;
 let mut count=pointer(0x6340);count[0u16]=visible[0u16].count;
"#
    } else {
        "halt(0x7e84);"
    };
    let vehicle_call = if vehicle_methods {
        r#"
 entity=g(c,5);let call=scene_vehicle_transfer::Call{entity,slot:g(c,1).lo_bits(),payload:g(c,2).lo_bits(),value:g(c,3)};
 status=scene_vehicle_transfer::dispatch(&scene,&mut store,&mut runtime,&world,&vehicle_context,&call);
"#
    } else {
        "halt(0x7e82);"
    };
    let ufo_setup = if ufos || projectile_ufo {
        r#"
 world.grid=spatial::Grid{page:119,origin:pointer(0xf2c0),shift_x:scene.header[14u16],shift_z:scene.header[15u16],items:pointer(0xd200),ranges:pointer(0xf200)};
 let ufo_cars:Buf<scene_ufos::Car,3>=Buf::new([ufo_car(0),ufo_car(1),ufo_car(2)]);
"#
    } else {
        ""
    };
    let ufo_context = if projectile_ufo {
        r#"
 let ufo_context=scene_ufos::Context{class:g(pointer(0x66d0),0),resources_present:pointer(0x66d2)[0u16]!=0,mesh:g(pointer(0x66d4),0),material:g(pointer(0x66d6),0),cars:scene_ufos::Cars{records:ufo_cars.as_array(),count:3,present:pointer(0x64d5)[0u16]!=0}};
"#
    } else if projectile_area {
        r#"
 let no_ufos:Buf<scene_ufos::Car,0>=Buf::new([]);
 let ufo_context=scene_ufos::Context{class:integer::from_i16(-1),resources_present:false,mesh:integer::from_i16(0),material:integer::from_i16(0),cars:scene_ufos::Cars{records:no_ufos.as_array(),count:0,present:false}};
"#
    } else {
        ""
    };
    let ufo_call = if ufos {
        r#"
 let request=scene_ufos::Spawn{class:g(pointer(0x6310),0),resources_present:pointer(0x6312)[0u16]!=0,mesh:g(pointer(0x6314),0),material:g(pointer(0x6316),0),target:g(c,1),emitter_present:g(c,2)!=integer::from_i16(0),emitter:g(c,3),destination:pointer(0x6318),cars:scene_ufos::Cars{records:ufo_cars.as_array(),count:3,present:pointer(0x6313)[0u16]!=0}};
 let spawned=scene_ufos::spawn(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&request);
 if spawned<integer::from_i16(0){status=spawned;}else{entity=spawned;}
"#
    } else {
        "halt(0x7e7e);"
    };
    let weapon_setup = if weapon_explosion {
        r#"
 let hit_cars:Buf<scene_explosions::HitVehicle,8>=Buf::new([hit_car(0),hit_car(1),hit_car(2),hit_car(3),hit_car(4),hit_car(5),hit_car(6),hit_car(7)]);
"#
    } else {
        ""
    };
    let weapon_call = if label == "scene-weapon-explosion" {
        r#"
 let start=scene_explosions::Spawn{class:g(pointer(0x2372),0),frame:g(c,5),position:pointer(0x2376),context:explosion::Context{resources:pointer(0x2368),seed:runtime.seed,sine:launch.context.tables.sine}};
 let request=scene_explosions::WeaponSpawn{start,kind:g(c,1).lo_bits(),stamp:g(c,2),camera:pointer(0x238a),materials:pointer(0x2380),has_contact:g(c,3)!=integer::from_i16(0),vehicle:pointer(0x23b1)[0u16],local_point:pointer(0x239e),world_point:pointer(0x23a4),velocity:pointer(0x23aa),vehicles:hit_cars.as_array(),vehicle_count:pointer(0x23bc)[0u16]};
 let spawned=scene_explosions::spawn_weapon(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&request);
 if spawned<integer::from_i16(0){status=spawned;}else{entity=spawned;}
"#
    } else {
        "halt(0x7e77);"
    };
    let explosion_tick_setup = if vehicle_views {
        r#"
 let explosion_cars:Buf<scene_explosions::Vehicle,8>=Buf::new([car(0),car(1),car(2),car(3),car(4),car(5),car(6),car(7)]);
"#
    } else {
        ""
    };
    let mine_explosion_call = if label == "scene-mine-explosion" {
        r#"
 let start=scene_explosions::Spawn{class:g(pointer(0x2372),0),frame:g(c,5),position:pointer(0x2376),context:explosion::Context{resources:pointer(0x2368),seed:runtime.seed,sine:launch.context.tables.sine}};
 let car=&hit_cars[pointer(0x23b1)[0u16]];
 let request=scene_explosions::MineSpawn{start,kind:g(c,1),owner:car.entity,velocity:car.velocity,stamp:g(c,2),camera:pointer(0x238a),materials:pointer(0x2380)};
 let spawned=scene_explosions::spawn_mine(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&request);
 if spawned<integer::from_i16(0){status=spawned;}else{entity=spawned;}
"#
    } else {
        "halt(0x7e78);"
    };
    let explosion_tick_call = if vehicle_views {
        r#"
 entity=g(c,3);
 let step=scene_explosions::Tick{call:scene_creation::Call{entity,slot:g(c,2).lo_bits()},dt:g(c,1),resources:pointer(0x2368),sine:launch.context.tables.sine,simulation:pointer(0x23bd)[0u16]!=0,vehicles:explosion_cars.as_array(),vehicle_count:pointer(0x23bc)[0u16]};
 status=scene_explosions::dispatch(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&step);
"#
    } else {
        "halt(0x7e76);"
    };
    let explosion_call = if explosions {
        r#"
 let start=scene_explosions::Spawn{class:g(pointer(0x2372),0),frame:g(c,2),position:pointer(0x2376),context:explosion::Context{resources:pointer(0x2368),seed:runtime.seed,sine:launch.context.tables.sine}};
 let spawned=scene_explosions::spawn(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&start);
 if spawned<integer::from_i16(0){status=spawned;}else{entity=spawned;}
"#
    } else {
        "halt(0x7e75);"
    };
    let explosion_publish = if explosions {
        r#"
 let saved_effect=read_dseg_page(2);unsafe{write_dseg_page(2,128);}
 p(pointer(0x9414),0,quads.count);p(pointer(0x9416),0,lines.count);
 copy(pointer(0x9a58),overflow,16);
 unsafe{write_dseg_page(2,saved_effect);}
"#
    } else {
        ""
    };
    let mine_setup = if mines || fire_mode {
        r#"
 world.grid=spatial::Grid{page:119,origin:pointer(0xf2c0),shift_x:scene.header[14u16],shift_z:scene.header[15u16],items:pointer(0xd200),ranges:pointer(0xf200)};
 let directory:Buf<u16,64>=Buf::new([0;64]);let mut indices=directory.as_array();let mut cell=0u16;while cell<64{indices[cell]=cell;cell+=1;}
 let previous=read_dseg_page(3);
 let mut map=ground::World::begin(static_assets::AssetBytes{first_sector:49152,sector_count:16384,byte_len_lo:0,byte_len_hi:32},integer::from_i16(0),indices);
 unsafe{write_dseg_page(3,previous);}
 let mut placement=scene_mines::Placement{owner:g(pointer(0x5ba0),0),position:pointer(0x5ba2),repeat:false,kind:integer::from_i16(0),frame:g(pointer(0xc0),0),class:g(pointer(0x8c),0),resources_present:meta[14u16]!=0,terrain_present:meta[15u16]!=0,resources:pointer(0x5b80),map:view_of(&mut map),tables:projectile::Tables{sine:launch.context.tables.sine,angle:launch.context.tables.angle}};
 let cars:Buf<scene_mines::Vehicle,1>=Buf::new([scene_mines::Vehicle{present:pointer(0xc2)[0u16]!=0,position:pointer(0x5bb0)}]);
"#
    } else {
        ""
    };
    let fire_setup = if fire_mode {
        r#"
 let fire_cars:Buf<projectile_sweep::Car,3>=Buf::new([flight_car(0),flight_car(1),flight_car(2)]);
 let row=pointer(0x6100);
 let mut fire=scene_fire::Fire{car:scene_fire::Car{player:pointer(0x6164)[0u16]!=0,inventory:row,cooldown:at(row,3),impact:at(row,5),collision_size:at(row,7),recoil_rate:at(row,9),pitch_rate:at(row,11),recoil_mode:at(row,13),boost_control:at(row,15),spin:at(row,27)},mode:integer::from_i16(0),launch:view_of(&mut launch),mine:view_of(&mut placement),boost_feedback:at(row,57),spin_shared:at(row,63),cars:projectile_sweep::Cars{records:fire_cars.as_array(),count:3,slots:pointer(0x64cc),present:pointer(0x64d5)[0u16]!=0},shot:pointer(0x6160)};
"#
    } else {
        ""
    };
    let fire_call = if fire_mode {
        r#"
 fire.mode=g(c,1);placement.frame=g(c,2);
 status=scene_fire::apply(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&mut fire);
 if status==integer::from_i16(0) && pointer(0x6160)[0u16]!=0{entity=g(pointer(0x6161),0);}
"#
    } else {
        "halt(0x7e85);"
    };
    let projectile_setup = if projectile_flight {
        r#"
 let directory:Buf<u16,64>=Buf::new([0;64]);let mut indices=directory.as_array();let mut cell=0u16;while cell<64{indices[cell]=cell;cell+=1;}
 let previous=read_dseg_page(3);
 let mut map=ground::World::begin(static_assets::AssetBytes{first_sector:49152,sector_count:16384,byte_len_lo:0,byte_len_hi:32},integer::from_i16(0),indices);
 unsafe{write_dseg_page(3,previous);}
 let flying_cars:Buf<projectile_sweep::Car,3>=Buf::new([flight_car(0),flight_car(1),flight_car(2)]);
"#
    } else {
        ""
    };
    let projectile_call = if projectile_flight {
        r#"
 let mut reflected=pointer(0x6690);reflected[0u16]=0;
 entity=g(c,3);let id=scene_lifecycle::object(&store,entity);
 let mut action=projectile_flight::zero();
 if id<integer::from_i16(0){status=id;}else{
  let step=scene_projectiles::Frame{object:id.lo_bits(),flight:projectile_flight::Frame{dt:g(c,1),number:g(c,2),entity,carrier_present:g(c,5).lo_bits()&1!=0,carrier_basis:pointer(0x6500),carrier_position:pointer(0x6512),carrier_released:g(c,5).lo_bits()&2!=0,cars:projectile_sweep::Cars{records:flying_cars.as_array(),count:3,slots:pointer(0x64cc),present:pointer(0x64d5)[0u16]!=0},overlay:pointer(0x6520)},context:launch.context,map:view_of(&mut map),terrain_present:pointer(0x6640)[0u16]!=0,resources_present:launch.resources_present};
  status=scene_projectiles::advance(&mut world,&weapons,&step,&mut action);
  let mut zero=pointer(0x6660);let mut i=0u16;while i<41{zero[i]=0;i+=1;}
  if status==integer::from_i16(0){
   pack_flight(pointer(0x6660),&action);
   let finish=scene_projectiles::Finish{call:scene_creation::Call{entity,slot:12},object:id.lo_bits(),dt:g(c,1),action:action.kind};
   status=scene_projectiles::finish(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&finish);
  }
 }
"#
    } else {
        "halt(0x7e7b);"
    };
    let reflection_call = if projectile_flight {
        r#"
 entity=g(c,3);let id=scene_lifecycle::object(&store,entity);
 let mut clear=pointer(0x6660);let mut i=0u16;while i<41{clear[i]=0;i+=1;}
 let mut reflected=pointer(0x6690);reflected[0u16]=0;
 if id<integer::from_i16(0){status=id;}else{
  let vehicle=g(c,1).lo_bits();let mut spin=pointer(0x6550);if vehicle<3{spin=at(spin,vehicle*2);}
  let request=scene_projectiles::Reflection{object:id.lo_bits(),vehicle,cars:projectile_sweep::Cars{records:flying_cars.as_array(),count:3,slots:pointer(0x64cc),present:pointer(0x64d5)[0u16]!=0},spin,context:launch.context,simulation:g(c,5)!=integer::from_i16(0)};
  let result=scene_projectiles::reflect(&mut world,&weapons,&request);
  if result<integer::from_i16(0){status=result;}else{reflected[0u16]=result.lo_bits();}
 }
"#
    } else {
        "halt(0x7e7c);"
    };
    let area_setup = if projectile_direct {
        r#"
 let parts:Buf<tank_hit::Part,6>=Buf::new([damage_part(0,0),damage_part(0,1),damage_part(1,0),damage_part(1,1),damage_part(2,0),damage_part(2,1)]);
 let mut part_rows=parts.as_array();
 let direct_cars:Buf<scene_projectile_impact::HitVehicle,3>=Buf::new([damage_car(0,view_of(&mut part_rows[0u16])),damage_car(1,view_of(&mut part_rows[2u16])),damage_car(2,view_of(&mut part_rows[4u16]))]);
 let damage=scene_projectile_impact::Damage{cars:direct_cars.as_array(),count:3,shell:pointer(0x66a0),homer:pointer(0x66b1),cup:params[60u16],temporary:temporary::State{core:pointer(0x9c24),rows:pointer(0x9c76)},present:pointer(0x66c0)[0u16]!=0};
 let area_cars:Buf<scene_projectile_impact::Car,8>=Buf::new([area_car(0),area_car(1),area_car(2),area_car(3),area_car(4),area_car(5),area_car(6),area_car(7)]);
 let area_hits:Buf<scene_explosions::HitVehicle,8>=Buf::new([area_hit(0),area_hit(1),area_hit(2),area_hit(3),area_hit(4),area_hit(5),area_hit(6),area_hit(7)]);
"#
    } else if projectile_area {
        r#"
 let no_damage:Buf<scene_projectile_impact::HitVehicle,0>=Buf::new([]);
 let damage=scene_projectile_impact::Damage{cars:no_damage.as_array(),count:0,shell:pointer(0x66a0),homer:pointer(0x66b1),cup:0,temporary:temporary::State{core:pointer(0x9c24),rows:pointer(0x9c76)},present:false};
 let area_cars:Buf<scene_projectile_impact::Car,8>=Buf::new([area_car(0),area_car(1),area_car(2),area_car(3),area_car(4),area_car(5),area_car(6),area_car(7)]);
 let area_hits:Buf<scene_explosions::HitVehicle,8>=Buf::new([area_hit(0),area_hit(1),area_hit(2),area_hit(3),area_hit(4),area_hit(5),area_hit(6),area_hit(7)]);
"#
    } else {
        ""
    };
    let area_call = if projectile_area {
        r#"
 entity=g(c,3);let id=scene_lifecycle::object(&store,entity);let mut action=projectile_flight::zero();
 if id<integer::from_i16(0){status=id;}else{
  let flight=scene_projectiles::Frame{object:id.lo_bits(),flight:projectile_flight::Frame{dt:g(c,1),number:g(c,2),entity,carrier_present:false,carrier_basis:pointer(0x6500),carrier_position:pointer(0x6512),carrier_released:false,cars:projectile_sweep::Cars{records:flying_cars.as_array(),count:3,slots:pointer(0x64cc),present:pointer(0x64d5)[0u16]!=0},overlay:pointer(0x6520)},context:launch.context,map:view_of(&mut map),terrain_present:pointer(0x6640)[0u16]!=0,resources_present:launch.resources_present};
  let impact=scene_projectile_impact::Impact{object:id.lo_bits(),action:view_of(&mut action),resources_present:launch.resources_present,terrain_present:pointer(0x6640)[0u16]!=0,simulation:pointer(0x23bd)[0u16]!=0,crater:g(pointer(0x6560),0),map:view_of(&mut map),marks:projectile_marks::State{core:pointer(0x9bee)},class:g(pointer(0x2372),0),resources:pointer(0x2368),camera:pointer(0x238a),materials:pointer(0x2380),frame:g(c,2),stamp:g(c,5),sine:launch.context.tables.sine,hits:area_hits.as_array(),cars:area_cars.as_array(),vehicle_count:pointer(0x23bc)[0u16],has_vehicles:pointer(0x64d5)[0u16]!=0,slots:pointer(0x64cc),camera_impulse:pointer(0x6312)};
  let mut tick=scene_projectile_impact::Tick{call:scene_creation::Call{entity,slot:12},flight,impact,damage,ufos:ufo_context};
  status=scene_projectile_impact::dispatch(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&mut tick);
 }
"#
    } else {
        "halt(0x7e7d);"
    };
    let mine_hit_setup = if mine_hit {
        r#"
 world.grid=spatial::Grid{page:119,origin:pointer(0xf2c0),shift_x:scene.header[14u16],shift_z:scene.header[15u16],items:pointer(0xd200),ranges:pointer(0xf200)};
 let hit_peers:Buf<scene_mines::HitPeer,9>=Buf::new([mine_peer(0),mine_peer(1),mine_peer(2),mine_peer(3),mine_peer(4),mine_peer(5),mine_peer(6),mine_peer(7),mine_peer(8)]);
"#
    } else {
        ""
    };
    let mine_hit_call = if mine_hit {
        r#"
 entity=g(c,3);
 let request=scene_mines::Contact{target:g(c,1).lo_bits(),frame:g(c,2),vehicle:mine_car(0),shield_tails:pointer(0x6300),peers:hit_peers.as_array(),peer_count:9,camera_impulse:pointer(0x6312),class:g(pointer(0x2372),0),stamp:g(c,5),resources:pointer(0x2368),sine:launch.context.tables.sine,camera:pointer(0x238a),materials:pointer(0x2380)};
 status=scene_mines::contact(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&request);
"#
    } else {
        "halt(0x7e79);"
    };
    let mine_tick = if mines {
        r#"
 placement.frame=g(c,2);entity=g(c,3);
 let tick=scene_mines::Tick{call:scene_creation::Call{entity:entity,slot:12},dt:g(c,1),placement:view_of(&mut placement),vehicles:cars.as_array(),vehicle_count:pointer(0xc4)[0u16],has_vehicles:pointer(0xc3)[0u16]!=0};
 status=scene_mines::dispatch(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&tick);
"#
    } else {
        "halt(0x7e74);"
    };
    let mine_call = if mines {
        r#"
 placement.kind=g(c,1);placement.repeat=g(c,2)!=integer::from_i16(0);
 let spawned=scene_mines::spawn(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&placement);
 if spawned<integer::from_i16(0){status=spawned;}else{entity=spawned;}
"#
    } else {
        "halt(0x7e73);"
    };
    let data: Value = serde_json::from_slice(&fs::read(fixture.join(format!("{label}.json")))?)?;
    let code = compile(
        label,
        &r#"
const CREATION:bool=$CREATION;
const AREA:bool=$AREA;const UFOPROJECTILE:bool=$UFOPROJECTILE;
fn ufo_car(i:u16)->scene_ufos::Car{
 if UFOPROJECTILE {
  let row=pointer(0x6100+i*64);let flight=pointer(0x6400+i*68);
  return scene_ufos::Car{present:g(row,0)!=integer::from_i16(0),player:g(row,2)!=integer::from_i16(0),slot:pointer(0x66e8)[i],disabled:flight[1u16]!=0,position:at(row,8),basis:at(flight,12)};
 }
 let row=pointer(0x6100+i*32);
 scene_ufos::Car{present:row[0u16]!=0,player:row[1u16]!=0,slot:row[2u16],disabled:row[3u16]!=0,position:at(row,4),basis:at(row,10)}
}
fn transfer_car(address:u16,index:u16)->scene_vehicle_transfer::Car {
 let row=pointer(address);
 let pose=vehicle_pose::State{physical_position:row,reference:at(row,6),base_position:at(row,24),body_basis:at(row,30),body_position:at(row,48),tracks:at(row,54),turret:at(row,102),gun:at(row,126),attachments:at(row,150),bottom_position:at(row,198),recoil:at(row,204)};
 let transfer=vehicle_transfer::Car{pose,checkpoint:at(row,208),passed:at(row,212),laps:at(row,214),controls:at(row,216),velocity:at(row,222),acceleration:at(row,228),angular:at(row,232),track_slip:at(row,238),temporary_timer:at(row,240),config:pointer(0x6a00),travel:pointer(0x6a70),environment:pointer(0x6a40)};
 scene_vehicle_transfer::Car{present:pointer(0x66f4)[index]!=0,transfer,flags:at(row,244)}
}
fn damage_part(i:u16,upper:u16)->tank_hit::Part{
 let row=pointer(0x6800+i*288);let head=at(row,80);let r=at(row,96);let b=21+upper*6;let pose=33+upper*24;
 tank_hit::Part{present:r[b]!=0,revision:at(row,55+upper*4),target:damage::Target{state:at(row,45),player:r[0u16]!=0,basis:at(r,pose),position:at(r,pose+18),selected:at(r,b+2),mesh:damage::Mesh{present:r[b+1]!=0,vertices:unsafe{pointer(0xc000+head[1+upper*2]).as_ptr().as_i16_array()},vertex_count:head[6+upper*2],indices:pointer(0xc000+head[10+upper]),normals:unsafe{pointer(0xc000+head[2+upper*2]).as_ptr().as_i16_array()},face_count:head[7+upper*2]}}}
}
fn damage_car(i:u16,parts:Array<tank_hit::Part>)->scene_projectile_impact::HitVehicle{
 let row=pointer(0x6800+i*288);let r=at(row,96);let car=pointer(0x6100+i*64);
 scene_projectile_impact::HitVehicle{present:g(car,0)!=integer::from_i16(0),geometry_page:129,car:tank_hit::Vehicle{player:r[0u16]!=0,visible:r[1u16]!=0,inventory:at(row,25),timer:at(row,21),velocity:at(car,20),angular:at(car,26),external:at(r,3),has_external:r[2u16]!=0,collision_size:at(car,46),hit_counts:at(row,29),smoke:at(row,41),parts,base_position:at(car,8),grip:at(row,64),temporary:at(row,72)}}
}
fn area_car(i:u16)->scene_projectile_impact::Car{
 let row=pointer(0x6100+i*64);
 scene_projectile_impact::Car{present:g(row,0)!=integer::from_i16(0),entity:g(row,1),player:g(row,2)!=integer::from_i16(0),visible:g(row,3)!=integer::from_i16(0),base_position:at(row,8),physical_position:at(row,14),velocity:at(row,20),angular:at(row,26),blast:at(row,32),marker:at(row,46)}
}
fn area_hit(i:u16)->scene_explosions::HitVehicle{
 let row=pointer(0x6100+i*64);
 scene_explosions::HitVehicle{present:g(row,0)!=integer::from_i16(0),entity:g(row,1),velocity:at(row,20),spin:at(row,48),has_body:g(row,25)!=integer::from_i16(0),radius:g(row,26).lo_bits() as i16}
}
fn flight_car(i:u16)->projectile_sweep::Car{
 let row=pointer(0x6400+i*68);
 let mut position=at(row,6);let mut velocity=at(row,48);if AREA{position=pointer(0x6108+i*64);velocity=pointer(0x6114+i*64);}
 projectile_sweep::Car{present:row[0u16]!=0,disabled:row[1u16]!=0,visible:row[2u16]!=0,ai_disabled:row[3u16]!=0,entity:g(at(row,4),0),position,basis:at(row,12),turret:at(row,30),velocity,bounds:at(row,54),radius:g(at(row,66),0)}
}
fn pack_flight(mut out:Array<u16>,a:&projectile_flight::Action){
 out[0u16]=a.kind;out[1u16]=0;out[14u16]=0;
 if a.has_mark{out[1u16]=1;copy(at(out,8),a.mark.as_array(),6);}
 if a.kind==4{copy(at(out,2),a.origin.as_array(),6);}
 if a.has_contact{out[14u16]=1;p(at(out,15),0,a.contact.vehicle);copy(at(out,17),a.contact.world.point.as_array(),6);copy(at(out,23),a.contact.world.normal.as_array(),6);copy(at(out,29),a.contact.local.point.as_array(),6);copy(at(out,35),a.contact.local.normal.as_array(),6);}
}

fn mine_car(i:u16)->scene_mines::HitVehicle{
 let row=pointer(0x6100+i*64);
 scene_mines::HitVehicle{entity:g(row,0),player:g(row,1)!=integer::from_i16(0),protection:at(row,4),physics:mine::Vehicle{base_position:at(row,6),velocity:at(row,12),angular:at(row,18),has_external:g(row,12)!=integer::from_i16(0),external:at(row,26),health:at(row,44),collision_size:at(row,46)},effect:at(row,48)}
}
fn mine_peer(i:u16)->scene_mines::HitPeer{
 let index=pointer(0x6318)[i];let mut row=pointer(0x6100);let mut present=false;
 if index<8{row=pointer(0x6100+index*64);present=true;}
 scene_mines::HitPeer{present,entity:g(row,0),effect:at(row,48)}
}

fn at(v:Array<u16>,w:u16)->Array<u16>{unsafe{v.as_ptr().add(w as i16).as_u16_array()}}
fn g(v:Array<u16>,i:u16)->fix32{attitude::get(v,i)}
fn p(v:Array<u16>,i:u16,x:fix32){attitude::put(v,i,x);}
fn pointer(address:u16)->Array<u16>{unsafe{Ptr::from_addr(address).as_u16_array()}}
fn copy(mut to:Array<u16>,from:Array<u16>,count:u16){let mut i=0u16;while i<count{to[i]=from[i];i+=1;}}
fn list(index:u16)->ordered::List{ordered::List{items:pointer(0x3000+index*1024),count:pointer(0xa0)[index],capacity:512}}
fn car(slot:u16)->scene_explosions::Vehicle{let row=pointer(0xd10+slot*27);scene_explosions::Vehicle{present:row[0u16]!=0,entity:g(at(row,1),0),position:at(row,3),basis:at(row,9)}}
fn hit_car(slot:u16)->scene_explosions::HitVehicle{let row=pointer(0x5b80+slot*13);scene_explosions::HitVehicle{present:row[0u16]!=0,entity:g(at(row,1),0),velocity:at(row,3),spin:at(row,9),has_body:row[11u16]!=0,radius:row[12u16]as i16}}
fn main(){
 let meta=pointer(0x80);let initial2=read_dseg_page(2);let initial3=read_dseg_page(3);
 unsafe{write_dseg_page(2,120);write_dseg_page(3,119);}
 let header=pointer(0x100);let ids=entities::State{header:header,rows:at(header,12),storage_capacity:512};
 let mut world=scene_world::World{colliders:pointer(0x8000),properties:pointer(0xc000),count:meta[0u16],fixed_count:meta[0u16],dynamic_page:122,dynamic_capacity:215,
 grid:spatial::Grid{page:65535,origin:pointer(0),shift_x:0,shift_z:0,items:pointer(0),ranges:pointer(0)},deformables:ordered::List{items:pointer(0),count:0,capacity:0}};
 let mut store=scene_lifecycle::Store{identities:ids,records:pointer(0x1300),object_count:meta[1u16],object_capacity:512,colliders:world.colliders,collider_stride:60,fixed_count:world.fixed_count,dynamic_page:122,
 generic_class:meta[3u16],obstacle_class:meta[4u16],limit:fix32::from_words(34464,1),free_objects:list(5),lap_objects:list(6),signs:list(7),enter:list(8),leave:list(9)};
 let mut runtime=scene_world::Runtime{ticks:lifecycle::Ticks{flags:pointer(0xe00),flag_count:meta[2u16],order:list(0),next:g(pointer(0xb0),0)},
 slot:pointer(0xb2),seed:pointer(0xb4),queues:lifecycle::Queues{notifications:list(1),destruction:list(2)},ordinary:list(3),special:list(4)};
 let mut quads=quad_pool::Pool{records:pointer(0x8000),exhausted:pointer(0x93f4),count:g(pointer(0x2364),0)};
 let mut lines=effect_pool::Pool{lines:pointer(0x9418),exhausted:pointer(0x9a68),count:g(pointer(0x2366),0),overflow:effect_pool::line_zero()};
 let overflow=unsafe{view_of(&mut lines.overflow).as_ptr().as_u16_array()};
 $EXPLOSION_SETUP
 let mut weapons=scene_creation::Weapons{directory:pointer(0x2c00),object_capacity:512,projectile_page:123,projectile_pages:2,mine_page:125,mine_pages:1,explosion_page:126,explosion_pages:2,ufo_page:130,ufo_pages:$UFO_PAGES,explosion_world:pointer(0x2300),blasts:explosion_blasts::view(pointer(0x9a78)),effect_page:128,quads:view_of(&mut quads),lines:view_of(&mut lines),has_effect_pool:pointer(0x237c)[0u16]!=0,world_class:meta[6u16],mine_order:ordered::List{items:pointer(0x5c00),count:pointer(0xaa)[0u16],capacity:512},mine_count:pointer(0xbc)};
 let sine:Buf<u16,4>=Buf::new([45056,320,40960,0]);let angle:Buf<u16,4>=Buf::new([45376,5,514,0]);
 let params=pointer(0x5b00);
 let context=projectile::Context{shared:projectile::shared(pointer(0x5a00)),resources:projectile::Resources{tuning:params,main:at(params,30),trail:at(params,42),homing_tail:at(params,54),cup:params[60u16]},tables:projectile::Tables{sine:unsafe{sine.as_array().as_ptr().as_i16_array()},angle:unsafe{angle.as_array().as_ptr().as_i16_array()}},seed:runtime.seed};
 let mut launch=scene_creation::Launch{owner:projectile::Owner{entity:g(at(params,61),0),turret:at(params,63),gun:at(params,81),velocity:at(params,87)},context:context,kind:0,class:g(at(meta,7),0),resources_present:meta[9u16]!=0,homing_mesh:g(at(meta,10),0)};
 let desc=pointer(0x90);let scene=scene_query::Scene::begin(static_assets::AssetBytes{first_sector:40960,sector_count:desc[1u16],byte_len_lo:desc[2u16],byte_len_hi:desc[3u16]});
 if !scene.valid{halt(0x7e70);}
 $UFO_SETUP
 $VEHICLE_SETUP
 $ACTOR_SETUP
 $OBSTACLE_SETUP
 $UFO_CONTEXT
 $MINE_SETUP
 $FIRE_SETUP
 $LAP_SETUP
 $PROJECTILE_SETUP
 $AREA_SETUP
 $MINE_HIT_SETUP
 $EXPLOSION_TICK_SETUP
 $WEAPON_SETUP
 let count=meta[5u16];let mut frame=0u16;
 while frame<count{
  let c=pointer(0x2400+frame*12);let operation=g(c,0).lo_bits();let mut status=integer::from_i16(0);let mut entity=integer::from_i16(0);let mut object=integer::from_i16(-1);
  if CREATION{
   store.limit=g(c,4);
   if operation==0{
    let created=scene_creation::create(&scene,&mut store,&mut runtime,&mut world,&mut weapons,g(c,1).lo_bits());
    if created<integer::from_i16(0){status=created;}else{entity=created;}
   }else if operation==3{
    launch.kind=g(c,1).lo_bits();let spawned=scene_creation::spawn_projectile(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&launch);
    if spawned<integer::from_i16(0){status=spawned;}else{entity=spawned;}
   }else if operation==4{$MINE_CALL
   }else if operation==5{$MINE_TICK
   }else if operation==6{$EXPLOSION_CALL
   }else if operation==7{$EXPLOSION_TICK_CALL
   }else if operation==8{$WEAPON_CALL
   }else if operation==20{$LAP_CALL
   }else if operation==19{$FIRE_CALL
   }else if operation==18{$ACTOR_CALL
   }else if operation==17{$OBSTACLE_CALL
   }else if operation==16{$VEHICLE_CALL
   }else if operation==15{
    let index=g(c,1).lo_bits();let base=pointer(0x6108+index*64);
    p(base,0,g(base,0)+g(c,2));p(base,2,g(base,2)+g(c,3));
    attitude::rotate(pointer(0x640c+index*68),0,1,g(c,5),launch.context.tables.sine);
   }else if operation==14{$UFO_CALL
   }else if operation==13{$AREA_CALL
   }else if operation==12{$REFLECTION_CALL
   }else if operation==11{$PROJECTILE_CALL
   }else if operation==10{$MINE_HIT_CALL
   }else if operation==9{$MINE_EXPLOSION_CALL
   }else{entity=g(c,3);status=scene_creation::dispatch(&scene,&mut store,&mut runtime,&mut world,&mut weapons,&scene_creation::Call{entity:entity,slot:g(c,2).lo_bits()});}
   if entities::handle_kind(&store.identities,entity)==1{object=entities::handle(&store.identities,entity);}
  }else if operation==0{
   let created=entities::create_record(&store.identities,store.limit);entity=created.id;
   if created.status!=0{status=integer::from_i16(-1);}
   else{
    if g(c,2)==integer::from_i16(0){if entities::bind_class(&store.identities,entity,g(c,1).lo_bits())!=0{halt(0x7e71);}}
    let allocated=scene_lifecycle::allocate_transient(&mut store,&mut world,&mut runtime,entity);
    if allocated.status==0{object=allocated.id;}else{status=integer::from_i16(-1);if allocated.status==4{status=integer::from_i16(-4);}}
   }
  }else{
   entity=g(c,3);let class=entities::class(&store.identities,entity);let saved=g(runtime.slot,0);p(runtime.slot,0,integer::from_i16(3));
   if class<integer::from_i16(0){status=integer::from_i16(-1);}
   else{status=scene_lifecycle::parent_base(&scene,&mut store,&mut runtime,entity,class.lo_bits());}
   if status==integer::from_i16(0){p(runtime.slot,0,saved);}
  }
  if read_dseg_page(2)!=120 || read_dseg_page(3)!=119{halt(0x7e72);}
  $EXPLOSION_PUBLISH
  let mut published=pointer(0x80);published[0u16]=world.count;published[1u16]=store.object_count;published[2u16]=runtime.ticks.flag_count;
  let mut counts=pointer(0xa0);counts[0u16]=runtime.ticks.order.count;counts[1u16]=runtime.queues.notifications.count;counts[2u16]=runtime.queues.destruction.count;counts[3u16]=runtime.ordinary.count;counts[4u16]=runtime.special.count;
  counts[5u16]=store.free_objects.count;counts[6u16]=store.lap_objects.count;counts[7u16]=store.signs.count;counts[8u16]=store.enter.count;counts[9u16]=store.leave.count;
  counts[10u16]=weapons.mine_order.count;
  p(pointer(0xb0),0,runtime.ticks.next);
  p(pointer(0xf0),0,entity);p(pointer(0xf2),0,status);p(pointer(0xf4),0,object);
  unsafe{Ptr::from_addr(0xff).write(0,frame+1);}frame+=1;
 }
 $MINE_FINISH
 unsafe{write_dseg_page(3,initial3);write_dseg_page(2,initial2);}halt(0x600d);
}
"#.replace("$LAP_SETUP",lap_setup).replace("$LAP_CALL",lap_call).replace("$FIRE_SETUP",fire_setup).replace("$FIRE_CALL",fire_call).replace("$ACTOR_SETUP",actor_setup).replace("$ACTOR_CALL",actor_call).replace("$OBSTACLE_SETUP",obstacle_setup).replace("$OBSTACLE_CALL",obstacle_call).replace("$VEHICLE_SETUP",&vehicle_setup).replace("$VEHICLE_CALL",vehicle_call).replace("$UFOPROJECTILE",if projectile_ufo {"true"}else{"false"}).replace("$UFO_PAGES",if has_ufos {"1"}else{"0"}).replace("$CREATION",if creation {"true"}else{"false"}).replace("$MINE_SETUP",mine_setup).replace("$PROJECTILE_SETUP",projectile_setup).replace("$UFO_CONTEXT",ufo_context).replace("$UFO_SETUP",ufo_setup).replace("$UFO_CALL",ufo_call).replace("$AREA_SETUP",area_setup).replace("$AREA_CALL",area_call).replace("$AREA",if projectile_area {"true"}else{"false"}).replace("$PROJECTILE_CALL",projectile_call).replace("$REFLECTION_CALL",reflection_call).replace("$MINE_HIT_SETUP",mine_hit_setup).replace("$MINE_HIT_CALL",mine_hit_call).replace("$MINE_CALL",mine_call).replace("$MINE_TICK",mine_tick).replace("$EXPLOSION_CALL",explosion_call).replace("$EXPLOSION_PUBLISH",explosion_publish).replace("$EXPLOSION_TICK_SETUP",explosion_tick_setup).replace("$EXPLOSION_TICK_CALL",explosion_tick_call).replace("$WEAPON_SETUP",weapon_setup).replace("$WEAPON_CALL",weapon_call).replace("$MINE_EXPLOSION_CALL",mine_explosion_call).replace("$EXPLOSION_SETUP",if explosions {"unsafe{write_dseg_page(2,128);}copy(overflow,pointer(0x9a58),16);unsafe{write_dseg_page(2,120);}"}else{""}).replace("$MINE_FINISH",if mines || projectile_flight || fire_mode {"map.terrain.finish();"}else{""}),
        output,
    )?;
    let cases = data["cases"].as_array().ok_or("scene allocation cases")?;
    let mut checked = 0;
    let mut calls = 0;
    let mut instructions = 0u64;
    let mut minimum_stack = u16::MAX;
    for (case_index, case) in cases.iter().enumerate() {
        let mut cpu = CpuV3Sim::default();
        cpu.set_boot_window(&code.words);
        let memory = cpu.physical_memory_mut();
        let initial = &case["initial"];
        let fixed = case["fixed_count"].as_u64().unwrap() as usize;
        assert!(fixed <= 273 && fixed + 215 <= 512);
        let scene = fs::read(fixture.join(case["scene"].as_str().unwrap()))?;
        for (i, pair) in scene.chunks_exact(2).enumerate() {
            memory[0x280000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
        }
        memory[0x90..0x94].copy_from_slice(&[
            40960,
            scene.len().div_ceil(128) as u16,
            scene.len() as u16,
            (scene.len() >> 16) as u16,
        ]);
        memory[0x80] = fixed as u16;
        memory[0x81] = initial["component_count"].as_u64().unwrap() as u16;
        let flags = initial["flags"].as_array().unwrap();
        memory[0x82] = flags.len() as u16;
        memory[0x83] = case["generic"].as_u64().unwrap() as u16;
        memory[0x84] = case["obstacle"].as_u64().unwrap() as u16;
        let controls = case["controls"].as_array().unwrap();
        memory[0x85] = controls.len() as u16;
        memory[0x86] = case["world_class"].as_u64().unwrap_or(0) as u16;
        if creation {
            let mut next_slot = [0usize; 5];
            for weapon in initial["weapons"].as_array().unwrap() {
                let id = weapon["id"].as_u64().unwrap() as usize;
                let kind = weapon["kind"].as_u64().unwrap() as usize;
                let slot = next_slot[kind];
                next_slot[kind] += 1;
                let (first, count, size, banks) = if kind == 1 {
                    (123, 25, 640, 2)
                } else if kind == 2 {
                    (125, 341, 48, 1)
                } else if kind == 4 {
                    (130, 190, 86, 1)
                } else {
                    (126, 23, 706, 2)
                };
                assert!(slot < banks * count);
                memory[0x2c00 + id * 2] = kind as u16;
                memory[0x2c01 + id * 2] = slot as u16;
                let base = (first + slot / count) * 16384 + (slot % count) * size;
                let header = u16s(&weapon["meta"]);
                let payload = u16s(&weapon["payload"]);
                memory[base..base + 30].copy_from_slice(&header);
                memory[base + 30..base + 30 + payload.len()].copy_from_slice(&payload);
            }
            memory[0x87..0x89]
                .copy_from_slice(&words(case["projectile_class"].as_i64().unwrap() as i32));
            memory[0x89] = u16::from(case["resources_present"].as_bool().unwrap());
            memory[0x8a..0x8c]
                .copy_from_slice(&words(case["homing_mesh"].as_u64().unwrap() as i32));
            let flight = u16s(&initial["flight"]);
            memory[0x5a00..0x5a00 + flight.len()].copy_from_slice(&flight);
            let launch = u16s(&case["launch"]);
            memory[0x5b00..0x5b00 + launch.len()].copy_from_slice(&launch);
            for (at, name) in [(0x2c0000, "sine.i16.bin"), (45376 * 64, "angle.i16.bin")] {
                let bytes = fs::read(fixture.join(name))?;
                for (i, pair) in bytes.chunks_exact(2).enumerate() {
                    memory[at + i] = u16::from_le_bytes([pair[0], pair[1]]);
                }
            }
            for page in 123..=127 {
                let used = if page == 125 {
                    341 * 48
                } else if page >= 126 {
                    23 * 706
                } else {
                    25 * 640
                };
                memory[page * 16384 + used..(page + 1) * 16384].fill(0x79ab);
            }
        }
        if explosions {
            memory[0x2300..0x2364].copy_from_slice(&u16s(&initial["explosion_world"]));
            let effects = u16s(&initial["blast_effects"]);
            assert_eq!(effects.len(), 7152);
            let base = 128 * 16384;
            memory[base..base + 6776].copy_from_slice(&effects[374..7150]);
            memory[base + 6776..base + 7150].copy_from_slice(&effects[..374]);
            memory[base
                + if projectile_direct {
                    7478
                } else if projectile_area {
                    7204
                } else {
                    7150
                }..base + 16384]
                .fill(0x79ab);
            memory[0x2364..0x2368].copy_from_slice(&effects[374 + 5140..374 + 5144]);
            memory[0x2368..0x2372].copy_from_slice(&u16s(&case["explosion_resources"]));
            memory[0x2372..0x2374]
                .copy_from_slice(&words(case["explosion_class"].as_i64().unwrap() as i32));
            memory[0x2376..0x237c].copy_from_slice(&u16s(&case["explosion_position"]));
            memory[0x237c] = u16::from(case["effect_pool_present"].as_bool().unwrap());
        }
        if vehicle_views {
            memory[0xd10..0xde8].copy_from_slice(&u16s(&case["vehicle_inputs"]));
            memory[0x23bc] = case["vehicle_count"].as_u64().unwrap() as u16;
            memory[0x23bd] = u16::from(case["simulation"].as_bool().unwrap());
        }
        if weapon_explosion {
            memory[0x5b80..0x5be8].copy_from_slice(&u16s(&case["hit_inputs"]));
            memory[0x2380..0x238a].copy_from_slice(&u16s(&case["blast_materials"]));
            memory[0x238a..0x239c].copy_from_slice(&u16s(&case["blast_camera"]));
            memory[0x239e..0x23b0].copy_from_slice(&u16s(&case["contact"]));
            memory[0x23b1] = case["hit_vehicle"].as_u64().unwrap() as u16;
        }
        if projectile_area {
            memory[0x6100..0x6300].copy_from_slice(&u16s(&initial["area_cars"]));
            memory[0x6312..0x6318].copy_from_slice(&u16s(&initial["camera_impulse"]));
            memory[0x6560..0x6562].copy_from_slice(&words(case["crater"].as_u64().unwrap() as i32));
            let base = 128 * 16384;
            memory[base + 7150..base + 7204].copy_from_slice(&u16s(&initial["marks"]));
            memory[base + 7204..base + 16384].fill(0x79ab);
            memory[0x2380..0x238a].copy_from_slice(&u16s(&case["blast_materials"]));
            memory[0x238a..0x239c].copy_from_slice(&u16s(&case["blast_camera"]));
        }
        if projectile_flight {
            memory[0x6400..0x64d6].copy_from_slice(&u16s(&case["flight_cars"]));
            memory[0x6500..0x6518].copy_from_slice(&u16s(&case["carrier_pose"]));
            memory[0x6520..0x6525].copy_from_slice(&u16s(&initial["overlay"]));
            memory[0x6640] = u16::from(case["terrain_present"].as_bool().unwrap());
            memory[0x6550..0x6556].copy_from_slice(&u16s(&case["spin_inputs"]));
            let cells = fs::read(fixture.join("projectile-cells.bin"))?;
            for (i, pair) in cells.chunks_exact(2).enumerate() {
                memory[0x300000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        if mines || fire_mode {
            memory[0x8c..0x8e].copy_from_slice(&words(case["mine_class"].as_i64().unwrap() as i32));
            memory[0x8e] = u16::from(case["mine_resources_present"].as_bool().unwrap());
            memory[0x8f] = u16::from(case["terrain_present"].as_bool().unwrap());
            memory[0xc0..0xc2]
                .copy_from_slice(&words(case["frame_group"].as_u64().unwrap() as i32));
            if let Some(position) = case.get("vehicle_position") {
                memory[0x5bb0..0x5bb6].copy_from_slice(&u16s(position));
                memory[0xc2] = u16::from(case["vehicle_present"].as_bool().unwrap());
                memory[0xc3] = u16::from(case["has_vehicles"].as_bool().unwrap());
                memory[0xc4] = case["vehicle_count"].as_u64().unwrap() as u16;
            }
            let resources = u16s(&case["mine_resources"]);
            assert_eq!(resources.len(), 18);
            memory[0x5b80..0x5b92].copy_from_slice(&resources);
            let owner = u16s(&case["mine_owner"]);
            assert_eq!(owner.len(), 8);
            memory[0x5ba0..0x5ba8].copy_from_slice(&owner);
            let cells = fs::read(fixture.join("projectile-cells.bin"))?;
            assert_eq!(cells.len(), 2097152);
            for (i, pair) in cells.chunks_exact(2).enumerate() {
                memory[0x300000 + i] = u16::from_le_bytes([pair[0], pair[1]]);
            }
        }
        if mine_membership {
            memory[0xbc..0xbe]
                .copy_from_slice(&words(initial["mine_count"].as_u64().unwrap() as i32));
            let order = initial["mine_order"].as_array().unwrap();
            memory[0xaa] = order.len() as u16;
            for (i, v) in order.iter().enumerate() {
                memory[0x5c00 + i * 2..0x5c02 + i * 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
            let base = 119 * 16384;
            for (i, cell) in initial["grid"].as_array().unwrap().iter().enumerate() {
                let cell = cell.as_array().unwrap();
                assert!(cell.len() <= 64);
                memory[base + 0x3200 + i * 3..base + 0x3203 + i * 3].copy_from_slice(&[
                    (i * 128) as u16,
                    cell.len() as u16,
                    64,
                ]);
                for (j, v) in cell.iter().enumerate() {
                    memory[base + 0x1200 + i * 128 + j * 2..base + 0x1202 + i * 128 + j * 2]
                        .copy_from_slice(&words(v.as_i64().unwrap() as i32));
                }
            }
            for i in 0..4 {
                memory[base + 0x32c0 + i] =
                    u16::from_le_bytes([scene[20 + i * 2], scene[21 + i * 2]]);
            }
        }
        if mine_hit {
            memory[0x6100..0x6300].copy_from_slice(&u16s(&initial["mine_cars"]));
            memory[0x6300..0x6312].copy_from_slice(&u16s(&initial["shield_tails"]));
            memory[0x6312..0x6318].copy_from_slice(&u16s(&initial["camera_impulse"]));
            memory[0x6318..0x6321].copy_from_slice(&u16s(&case["peer_indices"]));
            memory[0x2380..0x238a].copy_from_slice(&u16s(&case["blast_materials"]));
            memory[0x238a..0x239c].copy_from_slice(&u16s(&case["blast_camera"]));
        }
        if projectile_direct {
            memory[0x6800..0x6b60].copy_from_slice(&u16s(&initial["damage_profiles"]));
            memory[0x66a0..0x66b1].copy_from_slice(&u16s(&case["shell_hit"]));
            memory[0x66b1..0x66b9].copy_from_slice(&u16s(&case["homer_hit"]));
            memory[0x66c0] = u16::from(case["temporary_present"].as_bool().unwrap());
            let base = 128 * 16384;
            memory[base + 7204..base + 7478].copy_from_slice(&u16s(&initial["temporary"]));
            memory[base + 7478..base + 16384].fill(0x79ab);
            let base = 129 * 16384;
            memory[base..base + 16384].fill(0x79ab);
            let geometry = u16s(&initial["damage_geometry"]);
            assert!(geometry.len() < 16384);
            memory[base..base + geometry.len()].copy_from_slice(&geometry);
        }
        if laps {
            let bindings = u16s(&initial["lap_bindings"]);
            assert!(bindings.len() <= 0xc00);
            memory[0x6100..0x6100 + bindings.len()].copy_from_slice(&bindings);
            memory[0x6f8a] = (bindings.len() / 6) as u16;
            let backups = u16s(&initial["lap_backups"]);
            assert!(backups.len() <= 0x200);
            memory[0x6d00..0x6d00 + backups.len()].copy_from_slice(&backups);
            memory[0x6f89] = (backups.len() / 19) as u16;
            let attachments = u16s(&initial["lap_attachments"]);
            assert!(attachments.len() / 3 <= 16);
            memory[0x6f88] = (attachments.len() / 3) as u16;
            for (i, row) in attachments.chunks_exact(3).enumerate() {
                memory[0x6f40 + i] = row[0];
                memory[0x6f00 + i * 2..0x6f02 + i * 2].copy_from_slice(&row[1..]);
            }
            let deformables = u16s(&initial["lap_deformables"]);
            assert!(deformables.len() <= 256);
            memory[0x7000..0x7000 + deformables.len()].copy_from_slice(&deformables);
            memory[0x6f90] = (deformables.len() / 2) as u16;
            let cameras = u16s(&case["lap_cameras"]);
            assert!(cameras.len() <= 256);
            memory[0x7100..0x7100 + cameras.len()].copy_from_slice(&cameras);
        }
        if obstacle_methods {
            memory[0x66fc] = initial["colliders"].as_array().unwrap().len() as u16;
            memory[0x66e0..0x66e6].copy_from_slice(&u16s(&case["transfer_position"]));
            memory[0x66f0] = u16::from(case["shape_present"].as_bool().unwrap());
            memory[0x66f8..0x66fa].copy_from_slice(&u16s(&initial["transfer_result"]));
        }
        if vehicle_methods && !ufo_tick {
            for (base, name, limit) in [
                (0x6800, "transfer_car", 250),
                (0x66f8, "transfer_result", 2),
            ] {
                let v = u16s(&initial[name]);
                assert_eq!(v.len(), limit);
                memory[base..base + v.len()].copy_from_slice(&v);
            }
            for (base, name, limit) in [
                (0x6a00, "transfer_config", 54),
                (0x6a40, "transfer_environment", 36),
                (0x6a70, "transfer_travel", 8),
                (0x6b00, "transfer_order", 256),
                (0x6c00, "transfer_occupied", 256),
                (0x7000, "transfer_points", 1024),
            ] {
                let v = u16s(&case[name]);
                assert!(v.len() <= limit, "vehicle methods {name}");
                memory[base..base + v.len()].copy_from_slice(&v);
            }
            memory[0x66f0] = (u16s(&case["transfer_order"]).len() / 2) as u16;
            memory[0x66f1] = (u16s(&case["transfer_points"]).len() / 4) as u16;
            memory[0x66f2] = (u16s(&case["transfer_occupied"]).len() / 2) as u16;
            memory[0x66f3] = u16::from(case["vehicles_present"].as_bool().unwrap());
            memory[0x66f4] = u16::from(case["car_present"].as_bool().unwrap());
        }
        if fire_mode {
            memory[0x6100..0x6149].copy_from_slice(&u16s(&initial["fire_state"]));
            memory[0x6160..0x6163].copy_from_slice(&u16s(&initial["fire_shot"]));
            memory[0x6164] = u16::from(case["fire_player"].as_bool().unwrap());
            memory[0x6400..0x64d6].copy_from_slice(&u16s(&case["flight_cars"]));
        }
        if ufo_tick {
            let cars = u16s(&initial["transfer_cars"]);
            assert_eq!(cars.len(), 750);
            for (i, base) in [0x6800, 0x6900, 0x6e00].into_iter().enumerate() {
                memory[base..base + 250].copy_from_slice(&cars[i * 250..i * 250 + 250]);
                memory[0x66f4 + i] = u16::from(case["car_presence"][i].as_bool().unwrap());
            }
            for (base, name, limit) in [
                (0x6a00, "transfer_config", 54),
                (0x6a40, "transfer_environment", 36),
                (0x6a70, "transfer_travel", 8),
                (0x6b00, "transfer_order", 256),
                (0x7000, "transfer_points", 1024),
                (0x6100, "car_info", 24),
                (0x6120, "racers", 9),
                (0x6140, "shapes", 512),
            ] {
                let v = u16s(&case[name]);
                assert!(v.len() <= limit, "UFO actor {name}");
                memory[base..base + v.len()].copy_from_slice(&v);
            }
            for (base, name) in [
                (0x6c00, "transfer_occupied"),
                (0x6d00, "transfer_visible"),
                (0x6520, "overlay"),
            ] {
                let v = u16s(&initial[name]);
                assert!(v.len() <= 256);
                memory[base..base + v.len()].copy_from_slice(&v);
            }
            memory[0x66f0] = (u16s(&case["transfer_order"]).len() / 2) as u16;
            memory[0x66f1] = (u16s(&case["transfer_points"]).len() / 4) as u16;
            memory[0x66f2] = (u16s(&initial["transfer_occupied"]).len() / 2) as u16;
            memory[0x6340] = (u16s(&initial["transfer_visible"]).len() / 2) as u16;
            memory[0x66f3] = u16::from(case["vehicles_present"].as_bool().unwrap());
            memory[0x66fc] = initial["colliders"].as_array().unwrap().len() as u16;
            memory[0x6341] = u16::from(case["ufo_resources_present"].as_bool().unwrap());
            memory[0x6342..0x6344]
                .copy_from_slice(&words(case["ufo_height"].as_i64().unwrap() as i32));
            memory[130 * 16384 + 190 * 86..131 * 16384].fill(0x79ab);
        }
        if projectile_ufo {
            memory[0x66d0..0x66d2]
                .copy_from_slice(&words(case["ufo_class"].as_i64().unwrap() as i32));
            memory[0x66d2] = u16::from(case["ufo_resources_present"].as_bool().unwrap());
            memory[0x66d4..0x66d6]
                .copy_from_slice(&words(case["ufo_mesh"].as_u64().unwrap() as i32));
            memory[0x66d6..0x66d8]
                .copy_from_slice(&words(case["ufo_material"].as_u64().unwrap() as i32));
            memory[0x66e8..0x66eb].copy_from_slice(&u16s(&case["ufo_car_slots"]));
            memory[130 * 16384 + 190 * 86..131 * 16384].fill(0x79ab);
        }
        if ufos {
            memory[0x6100..0x6160].copy_from_slice(&u16s(&case["ufo_cars"]));
            memory[0x6310..0x6312]
                .copy_from_slice(&words(case["ufo_class"].as_i64().unwrap() as i32));
            memory[0x6312] = u16::from(case["ufo_resources_present"].as_bool().unwrap());
            memory[0x6313] = u16::from(case["vehicles_present"].as_bool().unwrap());
            memory[0x6314..0x6316]
                .copy_from_slice(&words(case["ufo_mesh"].as_u64().unwrap() as i32));
            memory[0x6316..0x6318]
                .copy_from_slice(&words(case["ufo_material"].as_u64().unwrap() as i32));
            memory[0x6318..0x631e].copy_from_slice(&u16s(&case["destination"]));
            memory[130 * 16384 + 190 * 86..131 * 16384].fill(0x79ab);
        }
        let identity = u16s(&initial["identity"]);
        assert!(identity.len() <= 12 + 512 * 6);
        memory[0x100..0x100 + identity.len()].copy_from_slice(&identity);
        for (i, v) in flags.iter().enumerate() {
            memory[0xe00 + i * 2..0xe02 + i * 2]
                .copy_from_slice(&words(v.as_u64().unwrap() as i32));
        }
        for (i, row) in initial["records"].as_array().unwrap().iter().enumerate() {
            memory[0x1300 + i * 8..0x1308 + i * 8].copy_from_slice(&u16s(row));
        }
        for (i, row) in initial["colliders"].as_array().unwrap().iter().enumerate() {
            let row = u16s(row);
            assert_eq!(row.len(), 76);
            let (collider, properties) = if i < fixed {
                (120 * 16384 + i * 60, 119 * 16384 + i * 16)
            } else {
                let base = 122 * 16384 + (i - fixed) * 76;
                (base, base + 60)
            };
            memory[collider..collider + 60].copy_from_slice(&row[..60]);
            memory[properties..properties + 16].copy_from_slice(&row[60..]);
        }
        for (i, list) in initial["lists"].as_array().unwrap().iter().enumerate() {
            let list = list.as_array().unwrap();
            assert!(list.len() <= 512);
            memory[0xa0 + i] = list.len() as u16;
            for (j, v) in list.iter().enumerate() {
                memory[0x3000 + i * 1024 + j * 2..0x3002 + i * 1024 + j * 2]
                    .copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        memory[0xb0..0xb2].copy_from_slice(&words(initial["next"].as_i64().unwrap() as i32));
        memory[0xb4..0xb6].copy_from_slice(&words(initial["random"].as_u64().unwrap() as i32));
        memory[0xb2..0xb4].copy_from_slice(&words(initial["slot"].as_u64().unwrap() as i32));
        for (frame, c) in controls.iter().enumerate() {
            for (i, v) in c.as_array().unwrap().iter().enumerate() {
                let at = 0x2400 + frame * 12 + i * 2;
                memory[at..at + 2].copy_from_slice(&words(v.as_i64().unwrap() as i32));
            }
        }
        assert!(controls.len() * 12 <= 0x800);
        let expected = case["frames"].as_array().unwrap();
        let mut done = 0;
        let mut passed = false;
        let mut started = false;
        for step in 1..=20000000 {
            let result = cpu
                .step()
                .map_err(|e| format!("allocation case {case_index}: {e:?}"))?;
            let sp = cpu.registers()[13];
            if started || sp != 0 {
                started = true;
                minimum_stack = minimum_stack.min(sp);
                if explosions
                    || has_ufos
                    || vehicle_methods
                    || obstacle_methods
                    || fire_mode
                    || laps
                {
                    assert!(
                        sp >= if vehicle_methods || laps {
                            0x7400
                        } else {
                            0x7000
                        },
                        "explosion factory stack crossed its guarded region"
                    );
                }
            }
            let memory = cpu.physical_memory_slice();
            if usize::from(memory[0xff]) > done {
                assert_eq!(usize::from(memory[0xff]), done + 1);
                let frame = &expected[done];
                let state = &frame["state"];
                let label = format!("allocation case {case_index} {} call {done}", case["tag"]);
                checked += check_words(
                    memory,
                    0xf0,
                    &words(frame["entity"].as_u64().unwrap() as i32),
                    &label,
                )?;
                checked += check_words(
                    memory,
                    0xb0,
                    &words(state["next"].as_i64().unwrap() as i32),
                    &format!("{label} tick cursor"),
                )?;
                checked += check_words(
                    memory,
                    0xb4,
                    &words(state["random"].as_u64().unwrap() as i32),
                    &format!("{label} RNG"),
                )?;
                checked += check_words(
                    memory,
                    0xf2,
                    &words(frame["status"].as_i64().unwrap() as i32),
                    &label,
                )?;
                checked += check_words(
                    memory,
                    0xf4,
                    &words(frame["object"].as_u64().map_or(-1, |v| v as i32)),
                    &label,
                )?;
                let identity = u16s(&state["identity"]);
                let mut canonical = memory[0x100..0x100 + identity.len()].to_vec();
                for row in canonical[12..].chunks_exact_mut(6) {
                    if row[0] & 2 == 0 {
                        row[1] = 0;
                    }
                    if (row[0] >> 2) & 7 == 0 {
                        row[2] = 0;
                        row[3] = 0;
                    }
                }
                checked += check_words(&canonical, 0, &identity, &format!("{label} identity"))?;
                checked += check_words(
                    memory,
                    0xb2,
                    &words(state["slot"].as_u64().unwrap() as i32),
                    &format!("{label} dispatch slot"),
                )?;
                for (i, v) in state["flags"].as_array().unwrap().iter().enumerate() {
                    checked += check_words(
                        memory,
                        0xe00 + i * 2,
                        &words(v.as_u64().unwrap() as i32),
                        &format!("{label} tick flags {i}"),
                    )?;
                }
                let rows = state["colliders"].as_array().unwrap();
                assert_eq!(
                    usize::from(memory[0x80]),
                    rows.len(),
                    "{label} collider count"
                );
                assert_eq!(
                    u64::from(memory[0x81]),
                    state["component_count"].as_u64().unwrap(),
                    "{label} component count"
                );
                assert_eq!(
                    usize::from(memory[0x82]),
                    state["flags"].as_array().unwrap().len(),
                    "{label} tick flag length"
                );
                for (i, row) in rows.iter().enumerate() {
                    let row = u16s(row);
                    let (collider, properties) = if i < fixed {
                        (120 * 16384 + i * 60, 119 * 16384 + i * 16)
                    } else {
                        let base = 122 * 16384 + (i - fixed) * 76;
                        (base, base + 60)
                    };
                    checked += check_words(
                        memory,
                        collider,
                        &row[..60],
                        &format!("{label} collider {i}"),
                    )?;
                    checked += check_words(
                        memory,
                        properties,
                        &row[60..],
                        &format!("{label} properties {i}"),
                    )?;
                    let mut metadata = memory[0x1300 + i * 8..0x1308 + i * 8].to_vec();
                    if metadata[7] & 1 == 0 {
                        metadata[4..7].fill(0);
                    }
                    checked += check_words(
                        &metadata,
                        0,
                        &u16s(&state["records"][i]),
                        &format!("{label} metadata {i}"),
                    )?;
                }
                for (i, list) in state["lists"].as_array().unwrap().iter().enumerate() {
                    let list = list.as_array().unwrap();
                    assert_eq!(
                        usize::from(memory[0xa0 + i]),
                        list.len(),
                        "{label} list {i}"
                    );
                    for (j, v) in list.iter().enumerate() {
                        checked += check_words(
                            memory,
                            0x3000 + i * 1024 + j * 2,
                            &words(v.as_i64().unwrap() as i32),
                            &format!("{label} list {i}"),
                        )?;
                    }
                }
                if creation {
                    checked += check_words(
                        memory,
                        0x5a00,
                        &u16s(&state["flight"]),
                        &format!("{label} shared flight"),
                    )?;
                    for page in 123..=127 {
                        let used = if page == 125 {
                            341 * 48
                        } else if page >= 126 {
                            23 * 706
                        } else {
                            25 * 640
                        };
                        assert!(
                            memory[page * 16384 + used..(page + 1) * 16384]
                                .iter()
                                .all(|&v| v == 0x79ab),
                            "{label} bank {page} guard"
                        );
                    }
                    let weapons = state["weapons"].as_array().unwrap();
                    let mut expected_live = vec![false; 512];
                    for w in weapons {
                        let id = w["id"].as_u64().unwrap() as usize;
                        expected_live[id] = true;
                        let kind = w["kind"].as_u64().unwrap() as u16;
                        assert_eq!(memory[0x2c00 + id * 2], kind, "{label} weapon {id} kind");
                        let slot = memory[0x2c01 + id * 2] as usize;
                        let (first, count, size) = if kind == 1 {
                            (123, 25, 640)
                        } else if kind == 2 {
                            (125, 341, 48)
                        } else if kind == 4 {
                            (130, 190, 86)
                        } else {
                            (126, 23, 706)
                        };
                        let base = (first + slot / count) * 16384 + (slot % count) * size;
                        checked += check_words(
                            memory,
                            base,
                            &u16s(&w["meta"]),
                            &format!("{label} weapon {id} scene data"),
                        )?;
                        checked += check_words(
                            memory,
                            base + 30,
                            &u16s(&w["payload"]),
                            &format!("{label} weapon {id} payload"),
                        )?;
                    }
                    for (id, &live) in expected_live.iter().enumerate() {
                        assert_eq!(
                            memory[0x2c00 + id * 2] != 0,
                            live,
                            "{label} weapon {id} presence"
                        );
                    }
                    let mut occupied = 0;
                    for (kind, first, banks, count, size) in [
                        (1, 123, 2, 25, 640),
                        (2, 125, 1, 341, 48),
                        (3, 126, 2, 23, 706),
                        (4, 130, if has_ufos { 1 } else { 0 }, 190, 86),
                    ] {
                        for slot in 0..banks * count {
                            let base = (first + slot / count) * 16384 + (slot % count) * size;
                            let owner = usize::from(memory[base]);
                            if owner != 0 {
                                assert!(owner <= 512, "{label} pool owner");
                                assert_eq!(
                                    memory[0x2c00 + (owner - 1) * 2],
                                    kind,
                                    "{label} orphaned component"
                                );
                                assert_eq!(
                                    usize::from(memory[0x2c01 + (owner - 1) * 2]),
                                    slot,
                                    "{label} component slot"
                                );
                                occupied += 1;
                            }
                        }
                    }
                    assert_eq!(occupied, weapons.len(), "{label} component count");
                }
                if projectile_ufo {
                    let basis = u16s(&state["ufo_car_basis"]);
                    for car in 0..3 {
                        checked += check_words(
                            memory,
                            0x640c + car * 68,
                            &basis[car * 18..car * 18 + 18],
                            &format!("{label} UFO car {car} basis"),
                        )?;
                    }
                    assert!(
                        memory[130 * 16384 + 190 * 86..131 * 16384]
                            .iter()
                            .all(|v| *v == 0x79ab),
                        "{label} UFO bank guard"
                    );
                }
                if ufos {
                    checked += check_words(
                        memory,
                        0x6100,
                        &u16s(&case["ufo_cars"]),
                        &format!("{label} UFO car inputs"),
                    )?;
                    assert!(
                        memory[130 * 16384 + 190 * 86..131 * 16384]
                            .iter()
                            .all(|v| *v == 0x79ab),
                        "{label} UFO bank guard"
                    );
                }
                if explosions {
                    checked += check_words(
                        memory,
                        0x2300,
                        &u16s(&state["explosion_world"]),
                        &format!("{label} explosion slots"),
                    )?;
                    let effects = u16s(&state["blast_effects"]);
                    let base = 128 * 16384;
                    checked += check_words(
                        memory,
                        base,
                        &effects[374..7150],
                        &format!("{label} effect pool"),
                    )?;
                    checked += check_words(
                        memory,
                        base + 6776,
                        &effects[..374],
                        &format!("{label} shared blasts"),
                    )?;
                    assert!(
                        memory[base
                            + if projectile_direct {
                                7478
                            } else if projectile_area {
                                7204
                            } else {
                                7150
                            }..base + 16384]
                            .iter()
                            .all(|&v| v == 0x79ab),
                        "{label} effect bank guard"
                    );
                }
                if mine_membership {
                    checked += check_words(
                        memory,
                        0xbc,
                        &words(state["mine_count"].as_u64().unwrap() as i32),
                        &format!("{label} mine count"),
                    )?;
                    let order = state["mine_order"].as_array().unwrap();
                    assert_eq!(
                        usize::from(memory[0xaa]),
                        order.len(),
                        "{label} mine order count"
                    );
                    for (i, v) in order.iter().enumerate() {
                        checked += check_words(
                            memory,
                            0x5c00 + i * 2,
                            &words(v.as_i64().unwrap() as i32),
                            &format!("{label} mine order"),
                        )?;
                    }
                    for (i, cell) in state["grid"].as_array().unwrap().iter().enumerate() {
                        let cell = cell.as_array().unwrap();
                        let base = 119 * 16384;
                        assert_eq!(
                            usize::from(memory[base + 0x3201 + i * 3]),
                            cell.len(),
                            "{label} grid {i} count"
                        );
                        for (j, v) in cell.iter().enumerate() {
                            checked += check_words(
                                memory,
                                base + 0x1200 + i * 128 + j * 2,
                                &words(v.as_i64().unwrap() as i32),
                                &format!("{label} grid {i} item {j}"),
                            )?;
                        }
                    }
                }
                if projectile_flight && !projectile_area {
                    checked += check_words(
                        memory,
                        0x6660,
                        &u16s(&frame["action"]),
                        &format!("{label} flight action"),
                    )?;
                    checked += check_words(
                        memory,
                        0x6690,
                        &[u16::from(frame["reflection"].as_bool().unwrap_or(false))],
                        &format!("{label} reflection"),
                    )?;
                    checked += check_words(
                        memory,
                        0x6550,
                        &u16s(&case["spin_inputs"]),
                        &format!("{label} live spin inputs"),
                    )?;
                    let overlay = u16s(&state["overlay"]);
                    let overlay_words = if overlay[0] == 0 { 1 } else { 5 };
                    checked += check_words(
                        memory,
                        0x6520,
                        &overlay[..overlay_words],
                        &format!("{label} overlay"),
                    )?;
                    checked += check_words(
                        memory,
                        0x6400,
                        &u16s(&case["flight_cars"]),
                        &format!("{label} live flight cars"),
                    )?;
                    checked += check_words(
                        memory,
                        0x6500,
                        &u16s(&case["carrier_pose"]),
                        &format!("{label} carrier pose"),
                    )?;
                }
                if projectile_area {
                    checked += check_words(
                        memory,
                        0x6100,
                        &u16s(&state["area_cars"]),
                        &format!("{label} area cars"),
                    )?;
                    checked += check_words(
                        memory,
                        0x6312,
                        &u16s(&state["camera_impulse"]),
                        &format!("{label} camera impulse"),
                    )?;
                    checked += check_words(
                        memory,
                        128 * 16384 + 7150,
                        &u16s(&state["marks"]),
                        &format!("{label} marks"),
                    )?;
                    let overlay = u16s(&state["overlay"]);
                    let len = if overlay[0] == 0 { 1 } else { 5 };
                    checked +=
                        check_words(memory, 0x6520, &overlay[..len], &format!("{label} overlay"))?;
                }
                if projectile_direct {
                    let profiles = u16s(&state["damage_profiles"]);
                    assert_eq!(profiles.len(), 864);
                    for car in 0..3 {
                        for field in 0..288 {
                            let at = if (9..15).contains(&field) {
                                0x6100 + car * 64 + 20 + field - 9
                            } else if (15..21).contains(&field) {
                                0x6100 + car * 64 + 26 + field - 15
                            } else if (23..25).contains(&field) {
                                0x6100 + car * 64 + 46 + field - 23
                            } else {
                                0x6800 + car * 288 + field
                            };
                            checked += check_words(
                                memory,
                                at,
                                &[profiles[car * 288 + field]],
                                &format!("{label} damage car {car} field {field}"),
                            )?;
                        }
                    }
                    checked += check_words(
                        memory,
                        128 * 16384 + 7204,
                        &u16s(&state["temporary"]),
                        &format!("{label} temporary"),
                    )?;
                    let geometry = u16s(&state["damage_geometry"]);
                    checked += check_words(
                        memory,
                        129 * 16384,
                        &geometry,
                        &format!("{label} damage geometry"),
                    )?;
                    assert!(
                        memory[129 * 16384 + geometry.len()..130 * 16384]
                            .iter()
                            .all(|v| *v == 0x79ab)
                    );
                }
                if mine_hit {
                    for (address, name) in [
                        (0x6100, "mine_cars"),
                        (0x6300, "shield_tails"),
                        (0x6312, "camera_impulse"),
                    ] {
                        checked += check_words(
                            memory,
                            address,
                            &u16s(&state[name]),
                            &format!("{label} {name}"),
                        )?;
                    }
                    for (address, name) in [
                        (0x2380, "blast_materials"),
                        (0x238a, "blast_camera"),
                        (0x6318, "peer_indices"),
                    ] {
                        checked += check_words(
                            memory,
                            address,
                            &u16s(&case[name]),
                            &format!("{label} {name} inputs"),
                        )?;
                    }
                }
                if laps {
                    for (address, name) in [
                        (0x6100, "lap_bindings"),
                        (0x6d00, "lap_backups"),
                        (0x7000, "lap_deformables"),
                    ] {
                        checked += check_words(
                            memory,
                            address,
                            &u16s(&state[name]),
                            &format!("{label} {name}"),
                        )?;
                    }
                    let expected = u16s(&state["lap_attachments"]);
                    for (i, row) in expected.chunks_exact(3).enumerate() {
                        checked += check_words(
                            memory,
                            0x6f40 + i,
                            &row[..1],
                            &format!("{label} attachment ID"),
                        )?;
                        checked += check_words(
                            memory,
                            0x6f00 + i * 2,
                            &row[1..],
                            &format!("{label} attachment flags"),
                        )?;
                    }
                    checked += check_words(
                        memory,
                        0x6f90,
                        &[(u16s(&state["lap_deformables"]).len() / 2) as u16],
                        &format!("{label} deformable count"),
                    )?;
                    checked += check_words(
                        memory,
                        0x7100,
                        &u16s(&case["lap_cameras"]),
                        &format!("{label} camera inputs"),
                    )?;
                }
                if fire_mode {
                    checked += check_words(
                        memory,
                        0x6100,
                        &u16s(&state["fire_state"]),
                        &format!("{label} fire state"),
                    )?;
                    checked += check_words(
                        memory,
                        0x6160,
                        &u16s(&state["fire_shot"]),
                        &format!("{label} fire shot"),
                    )?;
                    checked += check_words(
                        memory,
                        0x6400,
                        &u16s(&case["flight_cars"]),
                        &format!("{label} fire cars inputs"),
                    )?;
                }
                if ufo_tick {
                    let cars = u16s(&state["transfer_cars"]);
                    assert_eq!(cars.len(), 750);
                    for (i, base) in [0x6800, 0x6900, 0x6e00].into_iter().enumerate() {
                        checked += check_words(
                            memory,
                            base,
                            &cars[i * 250..i * 250 + 250],
                            &format!("{label} transfer car {i}"),
                        )?;
                    }
                    for (base, name) in [
                        (0x6c00, "transfer_occupied"),
                        (0x6d00, "transfer_visible"),
                        (0x6520, "overlay"),
                    ] {
                        let v = u16s(&state[name]);
                        if name != "overlay" {
                            let count = if name == "transfer_occupied" {
                                0x66f2
                            } else {
                                0x6340
                            };
                            assert_eq!(memory[count] as usize * 2, v.len(), "{label} {name} count");
                        }
                        checked += check_words(memory, base, &v, &format!("{label} {name}"))?;
                    }
                    for (base, name) in [
                        (0x6a00, "transfer_config"),
                        (0x6a40, "transfer_environment"),
                        (0x6a70, "transfer_travel"),
                        (0x6b00, "transfer_order"),
                        (0x7000, "transfer_points"),
                        (0x6100, "car_info"),
                        (0x6120, "racers"),
                        (0x6140, "shapes"),
                    ] {
                        checked += check_words(
                            memory,
                            base,
                            &u16s(&case[name]),
                            &format!("{label} {name} inputs"),
                        )?;
                    }
                    assert!(
                        memory[130 * 16384 + 190 * 86..131 * 16384]
                            .iter()
                            .all(|v| *v == 0x79ab),
                        "{label} UFO actor bank guard"
                    );
                }
                if obstacle_methods {
                    checked += check_words(
                        memory,
                        0x66f8,
                        &u16s(&state["transfer_result"]),
                        &format!("{label} transfer_result"),
                    )?;
                    checked += check_words(
                        memory,
                        0x66e0,
                        &u16s(&case["transfer_position"]),
                        &format!("{label} transfer_position inputs"),
                    )?;
                }
                if vehicle_methods && !ufo_tick {
                    for (base, name) in [(0x6800, "transfer_car"), (0x66f8, "transfer_result")] {
                        checked += check_words(
                            memory,
                            base,
                            &u16s(&state[name]),
                            &format!("{label} {name}"),
                        )?;
                    }
                    for (base, name) in [
                        (0x6a00, "transfer_config"),
                        (0x6a40, "transfer_environment"),
                        (0x6a70, "transfer_travel"),
                        (0x6b00, "transfer_order"),
                        (0x6c00, "transfer_occupied"),
                        (0x7000, "transfer_points"),
                    ] {
                        checked += check_words(
                            memory,
                            base,
                            &u16s(&case[name]),
                            &format!("{label} {name} inputs"),
                        )?;
                    }
                }
                if vehicle_views {
                    checked += check_words(
                        memory,
                        0xd10,
                        &u16s(&case["vehicle_inputs"]),
                        &format!("{label} live vehicle inputs"),
                    )?;
                }
                if weapon_explosion {
                    checked += check_words(
                        memory,
                        0x5b80,
                        &u16s(&case["hit_inputs"]),
                        &format!("{label} live hit inputs"),
                    )?;
                    for (address, name) in [
                        (0x2380, "blast_materials"),
                        (0x238a, "blast_camera"),
                        (0x239e, "contact"),
                    ] {
                        checked += check_words(
                            memory,
                            address,
                            &u16s(&case[name]),
                            &format!("{label} {name} inputs"),
                        )?;
                    }
                }
                done += 1;
                calls += 1;
            }
            if let StepOutcome::Halted { signal } = result {
                assert_eq!(signal, PASS, "allocation case {case_index}");
                assert_eq!(done, expected.len());
                instructions += step;
                passed = true;
                break;
            }
        }
        if !passed {
            return Err(format!("allocation case {case_index}: instruction bound").into());
        }
    }
    let mut report = json!({"cases":cases.len(),"calls":calls,"checked_u16_values":checked,"maximum_integer_error":0,"instruction_steps":instructions,
        "code_bytes":code.total_code_words()*2,"minimum_stack_word":minimum_stack,"dynamic_row_bytes":152,"dynamic_bank_bytes":32768,"dynamic_row_capacity":215,"scope":data["scope"],"backend":"instruction, real class/resource tables without GPU competition"});
    if explosions
        || projectile_flight
        || ufos
        || vehicle_methods
        || obstacle_methods
        || fire_mode
        || laps
    {
        report["maximum_stack_bytes"] = json!((0x8000 - usize::from(minimum_stack)) * 2);
    }
    if vehicle_methods {
        report["car_state_bytes"] = json!(500);
    }
    if explosions {
        report["explosion_record_bytes"] = json!(1412);
        report["explosion_slots_per_bank"] = json!(23);
        report["effect_pool_and_blast_bytes"] = json!(14300);
        report["explosion_world_bytes"] = json!(200);
    }
    if projectile_area {
        report["projectile_mark_bytes"] = json!(108);
        report["effect_stage_bytes"] = json!(if projectile_direct { 14956 } else { 14408 });
    }
    if has_ufos {
        report["ufo_record_bytes"] = json!(172);
        report["ufo_slots_per_bank"] = json!(190);
    }
    if projectile_direct {
        report["temporary_state_bytes"] = json!(548);
        report["maximum_damage_geometry_bytes"] = json!(
            cases
                .iter()
                .map(|case| case["initial"]["damage_geometry"].as_array().unwrap().len() * 2)
                .max()
                .unwrap_or(0)
        );
    }
    fs::write(
        output.join(format!("{label}-report.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
