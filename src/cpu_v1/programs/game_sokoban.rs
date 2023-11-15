use crate::cpu_v1::assembler::{Assembler, RegisterCommon, RegisterSpecial};
use crate::cpu_v1::cpu_v1_build_mix;
use crate::cpu_v1::devices::*;
use crate::{clock_tick, execute_gates};
use std::ops::Range;

const MAP_SIZE: usize = 8;
const TARGET_MAX: usize = 8;

// page 0:
const ADDR_PLAYER_X: usize = 1;
const ADDR_PLAYER_Y: usize = 2;
const ADDR_TARGET_COUNT: usize = 3;
const ADDR_GAME_STATE: usize = 4;

const PAGE_PALETTE: usize = 6;
const PAGE_TARGET_LIST: usize = 7; // max 8 pairs of xy
const PAGE_MAP: usize = 8; // [8, 16), map[y][x] on page(PAGE_MAP+y) addr(x)

#[repr(u8)]
enum GameState {
    Play = 0,
    Win = 1,
}

const TILE_WALL: u8 = 0b1000;
const TILE_PLAYER: u8 = 0b0100;
const TILE_BOX: u8 = 0b0010;
const TILE_TARGET: u8 = 0b0001;

const PALETTE: [Color; 16] = [
    Color::Silver,  // 0000 ground
    Color::Olive,   // 0001 target
    Color::Lime,    // 0010 box
    Color::Yellow,  // 0011 box+target
    Color::Aqua,    // 0100 player
    Color::Fuchsia, // 0101 player+target
    Color::Purple,  // 0110 X
    Color::Purple,  // 0111 X
    Color::Blue,    // 1000 wall
    Color::Purple,  // 1001 X
    Color::Purple,  // 1010 X
    Color::Purple,  // 1011 X
    Color::Purple,  // 1100 X
    Color::Purple,  // 1101 X
    Color::Purple,  // 1110 X
    Color::Purple,  // 1111 X
];
fn parse_tile(c: char) -> u8 {
    match c {
        '#' => TILE_WALL,
        '.' => 0, // ground
        'x' => TILE_PLAYER,
        'b' => TILE_BOX,
        'X' => TILE_PLAYER | TILE_TARGET,
        'B' => TILE_BOX | TILE_TARGET,
        'T' => TILE_TARGET,
        _ => unreachable!(),
    }
}

struct GameMap {
    start: (usize, usize),
    target_list: Vec<(usize, usize)>,
    map: [u8; MAP_SIZE * MAP_SIZE],
}
impl GameMap {
    fn parse(tiles: [&'static str; MAP_SIZE]) -> Self {
        let mut map = GameMap {
            start: (0, 0),
            target_list: vec![],
            map: [0; MAP_SIZE * MAP_SIZE],
        };

        for y in 0..MAP_SIZE {
            assert_eq!(tiles[y].len(), MAP_SIZE);
            for (x, c) in tiles[y].chars().enumerate() {
                let i = parse_tile(c);
                map.map[y * MAP_SIZE + x] = i;
                if i & TILE_PLAYER > 0 {
                    map.start = (x, y);
                }
                if i & TILE_TARGET > 0 {
                    map.target_list.push((x, y));
                }
            }
        }

        map
    }

    fn export_rom(&self) -> [u8; 256] {
        let mut r = [0; 256];

        r[ADDR_PLAYER_X] = self.start.0 as u8;
        r[ADDR_PLAYER_Y] = self.start.1 as u8;

        assert!(self.target_list.len() < TARGET_MAX);
        r[ADDR_TARGET_COUNT] = self.target_list.len() as u8;

        r[ADDR_GAME_STATE] = GameState::Win as u8;

        for i in 0..16 {
            r[PAGE_PALETTE * 16 + i] = PALETTE[i] as u8;
        }
        for (i, (x, y)) in self.target_list.iter().enumerate() {
            r[PAGE_TARGET_LIST * 16 + i * 2] = *x as u8;
            r[PAGE_TARGET_LIST * 16 + i * 2 + 1] = *y as u8;
        }
        for y in 0..MAP_SIZE {
            for x in 0..MAP_SIZE {
                r[(PAGE_MAP + y) * 16 + x] = self.map[y * MAP_SIZE + x];
            }
        }

        r
    }
}

#[test]
fn test_frame_sync() {
    #[rustfmt::skip]
        let map = GameMap::parse([
        "########",
        "#......#",
        "#......#",
        "#.x....#",
        "#...b..#",
        "#....T.#",
        "#......#",
        "########",
    ]);
    let rom = map.export_rom();

    println!("rom content:");
    print!("     ");
    for x in 0..16 {
        print!("{x:x}");
    }
    println!();
    for y in 0..16 {
        print!("{y:3x}: ");
        for x in 0..16 {
            print!("{:x}", rom[y * 16 + x]);
        }
        println!();
    }
    println!();

    set_rom_content(&rom);

    const INST_ADDR_INIT: Range<usize> = 0..3;
    const INST_ADDR_GAME_LOOP: Range<usize> = 3..4;
    const INST_ADDR_GAME_WIN: Range<usize> = 4..6; // TODO
    const INST_ADDR_GAME_PLAY: Range<usize> = 6..10; // TODO
    const INST_ADDR_RENDER: Range<usize> = 14..16;

    // init()
    // game_loop() {
    //     if GameState == Play {
    //         game_play()
    //     } else {
    //         game_win()
    //     }
    //     render()
    // }

    let mut asm = Assembler::new();

    asm.func_decl("init", INST_ADDR_INIT);
    asm.func_decl("game_loop", INST_ADDR_GAME_LOOP);
    asm.func_decl("game_win", INST_ADDR_GAME_WIN);
    asm.func_decl("game_play", INST_ADDR_GAME_PLAY);
    asm.func_decl("render", INST_ADDR_RENDER);

    asm.func_impl("init", |asm| init(asm));
    asm.func_impl("game_loop", |asm| game_loop(asm));
    asm.func_impl("game_win", |asm| game_win(asm));
    asm.func_impl("game_play", |asm| game_play(asm));
    asm.func_impl("render", |asm| render(asm));

    println!("asm:\n{}\n", asm.to_pretty_string());

    start_emulation(asm);
}

fn start_emulation(asm: Assembler) {
    let inst = asm.finish();

    let (state, _state_ref) = cpu_v1_build_mix(inst);

    loop {
        let pc = state.pc.out.get_u8();
        if pc as usize >= inst.len() {
            break;
        }
        let inst_desc = inst[pc as usize];
        println!("pc {:08b}: inst {}", pc, inst_desc.to_string());

        execute_gates();

        clock_tick();
    }
}

use crate::cpu_v1::isa::RegisterIndex::*;

/// set devices, set gamepad mode, set palette
fn init(asm: &mut Assembler) {
    /// assuming rom cursor=0
    fn copy_rom(asm: &mut Assembler) {
        asm.reg0().load_imm(DeviceType::Rom as u8);
        asm.reg0().set_bus_addr0();
        asm.reg0().load_imm(0);
        asm.bus0(DeviceRomOpcode::SetCursorHigh as u8);
        asm.bus0(DeviceRomOpcode::SetCursorLow as u8);
        asm.reg3().assign_from(Reg0); // reg3 <- 0 (reg3 saves page index)
        asm.reg1().assign_from(Reg0); // reg1 <- 0 (addr in page)

        asm.comment("start copy rom to mem".to_string());
        let page_loop = asm.reg0().assign_from(Reg3);
        asm.reg0().set_mem_page();
        asm.reg3().inc();

        let inner_loop = asm.bus0(DeviceRomOpcode::ReadNext as u8); // reg0 <- rom[cursor++]
        asm.reg0().store_mem_reg(); // mem[page][reg1] <- reg0
        asm.reg1().inc();
        asm.jne_back(inner_loop); // jmp to inner loop if reg1 != 0 (overflow)

        asm.reg3().assign_from(Reg3); // set flags of reg3
        asm.jne_back(page_loop); // jmp to page loop if reg3 != 0 (overflow)
    }

    /// assuming graphics on bus_addr1
    fn load_palette(asm: &mut Assembler) {
        asm.comment("load palette".to_string());
        asm.reg0().load_imm(PAGE_PALETTE as u8);
        asm.reg0().set_mem_page(); // set page <- reg0
        asm.reg0().load_imm(0);
        asm.reg1().assign_from(Reg0);
        // do { load_mem; set_palette; inc(reg1); } while reg1!=0 (overflow)
        let loop_start = asm.reg0().load_mem_reg(); // reg0 = mem[page][reg1]
        asm.bus1(DeviceGraphicsV1Opcode::SetPalette as u8);
        asm.reg1().inc();
        asm.jne_back(loop_start);
    }

    // copy all from rom
    copy_rom(asm);

    // setup devices, gamepad on addr0, graphics on addr1
    asm.comment("init gamepad and graphics".to_string());
    asm.reg0().load_imm(DeviceType::Gamepad as u8);
    asm.reg0().set_bus_addr0();
    asm.reg0().load_imm(DeviceType::GraphicsV1 as u8);
    asm.reg0().set_bus_addr1();

    asm.reg0().load_imm(MAP_SIZE as u8);
    asm.reg1().assign_from(Reg0);
    asm.bus1(DeviceGraphicsV1Opcode::Resize as u8);

    asm.reg0().load_imm(ButtonQueryMode::Down as u8);
    asm.bus0(DeviceGamepadOpcode::SetButtonQueryMode as u8);

    // load palette to graphics
    load_palette(asm);
}

/// game loop, assuming graphics on bus1, bus0 unknown
fn game_loop(asm: &mut Assembler) {
    asm.comment("game loop start".to_string());
    // lock last frame input
    asm.reg0().load_imm(DeviceType::Gamepad as u8);
    asm.reg0().set_bus_addr0();
    asm.bus0(DeviceGamepadOpcode::NextFrame as u8);

    asm.comment("  read game state".to_string());
    asm.reg0().load_imm(0);
    asm.reg0().set_mem_page();
    asm.reg0().load_mem_imm(ADDR_GAME_STATE as u8);
    let win = asm.jne_forward();
    asm.comment("if GameState==Play => jmp to game_play".to_string());
    asm.jmp_long("game_play"); // GameState == Play
    asm.resolve_jmp(win);
    asm.comment("if GameState==Win => jmp to game_win".to_string());
    asm.jmp_long("game_win"); // GameState == Win
}

/// helper function to read map tile
/// x in reg1, y in reg2
/// return tile in reg0, x in reg1, y in reg2, mem page loaded
fn read_map_tile(asm: &mut Assembler) {
    // page = PAGE_MAP + y;
    // addr = x;
    asm.reg0().load_imm(PAGE_MAP as u8);
    asm.reg0().add_assign(Reg2);
    asm.reg0().set_mem_page();
    asm.reg0().load_mem_reg(); // reg0 = map[y][x]
}
/// helper function to write map tile
/// tile in reg0, x in reg1, y in reg2
fn write_map_tile(asm: &mut Assembler, skip_set_page: bool) {
    if !skip_set_page {
        asm.reg3().assign_from(Reg0);
        asm.reg0().load_imm(PAGE_MAP as u8);
        asm.reg0().add_assign(Reg2);
        asm.reg0().set_mem_page();
        asm.reg0().assign_from(Reg3);
    }
    asm.reg0().store_mem_reg(); // map[y][x] = reg0
}

/// gamepad on bus0
fn game_win(asm: &mut Assembler) {
    asm.reg0().load_imm(ButtonQueryType::ButtonStart as u8);
    asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
    let pressed = asm.jne_forward();
    let not_pressed = asm.jmp_forward();
    asm.resolve_jmp(pressed);
    asm.comment("if start pressed -> init".to_string());
    asm.jmp_long("init");
    asm.comment("if start not pressed".to_string());
    asm.resolve_jmp(not_pressed);

    // read player xy
    asm.reg0().xor_assign(Reg0);
    asm.reg0().set_mem_page();
    asm.reg0().load_mem_imm(ADDR_PLAYER_X as u8);
    asm.reg1().assign_from(Reg0);
    asm.reg0().load_mem_imm(ADDR_PLAYER_Y as u8);
    asm.reg2().assign_from(Reg0);

    // flash player tile to indicate GameState::Win
    read_map_tile(asm);
    asm.reg0().inc();
    write_map_tile(asm, true);

    asm.jmp_long("render");
}

/// gamepad on bus0
fn game_play(asm: &mut Assembler) {
    /// read input, return dx in reg2, dy in reg3
    fn read_input(asm: &mut Assembler) {
        // reg2: dx, reg3: dy
        asm.reg2().xor_assign(Reg2); // reg2 = 0
        asm.reg3().xor_assign(Reg3); // reg3 = 0

        // up -> dy -= 1
        asm.reg0().load_imm(ButtonQueryType::ButtonUp as u8);
        asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
        asm.reg0().neg();
        asm.reg3().add_assign(Reg0);
        // down -> dy += 1
        asm.reg0().load_imm(ButtonQueryType::ButtonDown as u8);
        asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
        asm.reg3().add_assign(Reg0);
        // left -> dx -= 1
        asm.reg0().load_imm(ButtonQueryType::ButtonLeft as u8);
        asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
        asm.reg0().neg();
        asm.reg2().add_assign(Reg0);
        // right -> dx += 1
        asm.reg0().load_imm(ButtonQueryType::ButtonRight as u8);
        asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
        asm.reg2().add_assign(Reg0);
    }

    // read input
    read_input(asm);

    // TODO calculate p+1, p+2 position, check bounds (highest bit)
    // TODO read p+1, p+2 map tile to memory? or in reg?
    // TODO try push box, write memory
    // TODO move player, write memory

    // TODO check targets -> set game state

    asm.jmp_long("render");
}

/// read map tiles, call graphics set pixel
/// assuming graphics on bus_addr1 and frame initialized
fn render(asm: &mut Assembler) {
    asm.reg0().load_imm(0);
    asm.reg1().assign_from(Reg0);
    asm.comment("reset graphics cursor".to_string());
    asm.bus1(DeviceGraphicsV1Opcode::SetCursor as u8);

    asm.comment("reg0: color, reg1: addr, reg3: page".to_string());
    asm.reg0().load_imm(PAGE_MAP as u8);
    asm.reg3().assign_from(Reg0);

    let page_loop = asm.reg0().assign_from(Reg3);
    asm.reg0().set_mem_page();
    asm.reg3().inc();
    asm.reg1().xor_assign(Reg1);
    let skip_jmp_back = asm.jmp_forward();
    let page_loop_mid = asm.jmp_back(page_loop); // page_loop is out of range, insert a mid point
    asm.resolve_jmp(skip_jmp_back);
    let inner_loop = asm.reg0().load_mem_reg(); // reg0 = mem[page][reg1]
    asm.bus1(DeviceGraphicsV1Opcode::SetColorNext as u8);
    asm.reg1().inc();
    asm.jg_back(inner_loop);

    asm.reg3().assign_from(Reg3); // set flags of reg3
    asm.jne_back(page_loop_mid); // jmp to page loop if reg3 != 0 (overflow)

    asm.comment("finish frame".to_string());
    asm.bus1(DeviceGraphicsV1Opcode::WaitNextFrame as u8);
    asm.bus1(DeviceGraphicsV1Opcode::PresentFrame as u8);

    asm.jmp_long("game_loop");
}
