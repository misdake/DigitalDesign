use crate::cpu_v1::assembler::{Assembler, RegisterCommon, RegisterSpecial};
use crate::cpu_v1::cpu_v1_build_mix;
use crate::cpu_v1::devices::*;
use crate::{clock_tick, execute_gates};
use std::ops::Range;

const MAP_SIZE: usize = 8;
const TARGET_MAX: usize = 8;

const ADDR_PLAYER_X: usize = 1;
const ADDR_PLAYER_Y: usize = 2;
const ADDR_GAME_STATE: usize = 3;

const ADDR_N1_X: u8 = 5;
const ADDR_N1_Y: u8 = 6;
const ADDR_N2_X: u8 = 7;
const ADDR_N2_Y: u8 = 8;
const ADDR_N1_BOX: u8 = 9;
const ADDR_N1_GROUND: u8 = 10;
const ADDR_N2_BOX: u8 = 11;
const ADDR_N2_GROUND: u8 = 12;
const ADDR_N1_TILE: u8 = 13;
const ADDR_N2_TILE: u8 = 14;

const PAGE_GAME: usize = 2;
const PAGE_PALETTE: usize = 3;
const PAGE_TARGET_LIST: usize = 4; // max 8 pairs of xy

// alloc [6,16)+[0,1) for map, so that out of bound coordinates will read 0 from map!
const PAGE_MAP: usize = 8; // [8, 16) + padding=2, map[y][x] on page(PAGE_MAP+y) addr(x)

#[repr(u8)]
enum GameState {
    Play = 0,
    Win = 1,
}

const TILE_GROUND: u8 = 0b1000;
const TILE_PLAYER: u8 = 0b0100;
const TILE_BOX: u8 = 0b0010;
const TILE_TARGET: u8 = 0b0001;

const PALETTE: [Color; 16] = [
    Color::Blue,    // 1000 wall
    Color::Purple,  // 1001 X
    Color::Purple,  // 1010 X
    Color::Purple,  // 1011 X
    Color::Purple,  // 1100 X
    Color::Purple,  // 1101 X
    Color::Purple,  // 1110 X
    Color::Purple,  // 1111 X
    Color::Silver,  // 0000 ground
    Color::Olive,   // 0001 target
    Color::Lime,    // 0010 box
    Color::Yellow,  // 0011 box+target
    Color::Aqua,    // 0100 player
    Color::Fuchsia, // 0101 player+target
    Color::Purple,  // 0110 X
    Color::Purple,  // 0111 X
];
fn parse_tile(c: char) -> u8 {
    match c {
        '#' => 0, // wall
        '.' => TILE_GROUND,
        'x' => TILE_GROUND | TILE_PLAYER,
        'b' => TILE_GROUND | TILE_BOX,
        'X' => TILE_GROUND | TILE_PLAYER | TILE_TARGET,
        'B' => TILE_GROUND | TILE_BOX | TILE_TARGET,
        'T' => TILE_GROUND | TILE_TARGET,
        _ => unreachable!(),
    }
}

struct GameMap {
    start: (usize, usize),
    map: [u8; MAP_SIZE * MAP_SIZE],
}
impl GameMap {
    fn parse(tiles: [&'static str; MAP_SIZE]) -> Self {
        let mut map = GameMap {
            start: (0, 0),
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
            }
        }

        map
    }

    fn export_rom(&self) -> [u8; 256] {
        let mut r = [0; 256];

        r[PAGE_GAME * 16 + ADDR_PLAYER_X] = self.start.0 as u8;
        r[PAGE_GAME * 16 + ADDR_PLAYER_Y] = self.start.1 as u8;

        r[PAGE_GAME * 16 + ADDR_GAME_STATE] = GameState::Play as u8;

        for i in 0..16 {
            r[PAGE_PALETTE * 16 + i] = PALETTE[i] as u8;
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
    let levels: Vec<[&str;8]> = vec![
        [
            "########",
            "###T####",
            "###.####",
            "###b.bT#",
            "#T.bx###",
            "####b###",
            "####T###",
            "########",
        ],
        [
            "########",
            "###....#",
            "###bbb.#",
            "#x.bTT.#",
            "#.bTTT##",
            "####..##",
            "########",
            "########",
        ],
    ];

    #[rustfmt::skip]
    let map = GameMap::parse(levels[1]);
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

    const INST_ADDR_INIT: Range<usize> = 0..2;
    const INST_ADDR_GAME_LOOP: Range<usize> = 2..3;
    const INST_ADDR_GAME_WIN: Range<usize> = 3..4;
    const INST_ADDR_GAME_PLAY: Range<usize> = 4..14;
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
        // let inst_desc = inst[pc as usize];
        // let comment = asm.get_comment(pc).map_or("", |s| s);
        // println!(
        //     "pc {} {:04b}: inst {} {}",
        //     pc / 16,
        //     pc % 16,
        //     inst_desc.to_string(),
        //     comment
        // );

        execute_gates();

        clock_tick();

        // let get_game_mem =
        //     |addr: u8| -> u8 { state.mem[PAGE_GAME * 16 + addr as usize].out.get_u8() };
        // println!("Reg: {:?}", state.reg.map(|r| r.out.get_u8()));
        // println!("Player X: {}", get_game_mem(ADDR_PLAYER_X as u8));
        // println!("Player Y: {}", get_game_mem(ADDR_PLAYER_Y as u8));
        // println!("ADDR_N1_X: {}", get_game_mem(ADDR_N1_X));
        // println!("ADDR_N1_Y: {}", get_game_mem(ADDR_N1_Y));
        // println!("ADDR_N2_X: {}", get_game_mem(ADDR_N2_X));
        // println!("ADDR_N2_Y: {}", get_game_mem(ADDR_N2_Y));
        // println!("ADDR_N1_BOX: {}", get_game_mem(ADDR_N1_BOX));
        // println!("ADDR_N1_GROUND: {}", get_game_mem(ADDR_N1_GROUND));
        // println!("ADDR_N2_BOX: {}", get_game_mem(ADDR_N2_BOX));
        // println!("ADDR_N2_GROUND: {}", get_game_mem(ADDR_N2_GROUND));
        // println!("ADDR_N1_TILE: {}", get_game_mem(ADDR_N1_TILE));
        // println!("ADDR_N2_TILE: {}", get_game_mem(ADDR_N2_TILE));
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
        // asm.bus0(DeviceRomOpcode::SetCursorLow as u8); // always 0
        asm.reg3().xor_assign(Reg3); // reg3 <- 0 (reg3 saves page index)
        asm.reg1().xor_assign(Reg1); // reg1 <- 0 (addr in page)

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

    asm.comment("Enter -> Restart".to_string());
    asm.reg0().load_imm(ButtonQueryType::ButtonStart as u8);
    asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
    asm.if_is_1_to_7(
        |asm| {
            asm.jmp_long("init");
        },
        |_| {},
    );

    asm.comment("  read game state".to_string());
    asm.reg0().load_imm(PAGE_GAME as u8);
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

fn game_win(asm: &mut Assembler) {
    // read player xy
    asm.reg0().load_imm(PAGE_GAME as u8);
    asm.reg0().set_mem_page();
    asm.reg0().load_mem_imm(ADDR_PLAYER_X as u8);
    asm.reg1().assign_from(Reg0);
    asm.reg0().load_mem_imm(ADDR_PLAYER_Y as u8);
    asm.reg2().assign_from(Reg0);

    // flash player tile to indicate GameState::Win
    read_map_tile(asm);
    asm.reg0().inc();
    asm.reg0().store_mem_reg();

    asm.jmp_long("render");
}

/// gamepad on bus0
fn game_play(asm: &mut Assembler) {
    /// read input, return any: reg1, dx in reg2, dy in reg3
    fn read_input(asm: &mut Assembler) {
        asm.comment("read_input".to_string());
        // reg1: any, reg2: dx, reg3: dy
        asm.reg1().xor_assign(Reg1); // reg1 = 0
        asm.reg2().xor_assign(Reg2); // reg2 = 0
        asm.reg3().xor_assign(Reg3); // reg3 = 0

        // up -> dy -= 1
        asm.reg0().load_imm(ButtonQueryType::ButtonUp as u8);
        asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
        asm.reg1().or_assign(Reg0);
        asm.reg0().neg();
        asm.reg3().add_assign(Reg0);
        // down -> dy += 1
        asm.reg0().load_imm(ButtonQueryType::ButtonDown as u8);
        asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
        asm.reg1().or_assign(Reg0);
        asm.reg3().add_assign(Reg0);
        // left -> dx -= 1
        asm.reg0().load_imm(ButtonQueryType::ButtonLeft as u8);
        asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
        asm.reg1().or_assign(Reg0);
        asm.reg0().neg();
        asm.reg2().add_assign(Reg0);
        // right -> dx += 1
        asm.reg0().load_imm(ButtonQueryType::ButtonRight as u8);
        asm.bus0(DeviceGamepadOpcode::QueryButton as u8);
        asm.reg1().or_assign(Reg0);
        asm.reg2().add_assign(Reg0);
    }

    /// input: reg2: dx, reg3: dy
    /// output: reg2: n1, reg3: n2
    fn read_n1_n2(asm: &mut Assembler) {
        asm.comment("read_n1_n2".to_string());
        asm.reg0().load_imm(PAGE_GAME as u8);
        asm.reg0().set_mem_page();

        // write n1x, n2x to memory
        asm.reg0().load_mem_imm(ADDR_PLAYER_X as u8);
        asm.reg0().add_assign(Reg2);
        asm.reg0().store_mem_imm(ADDR_N1_X);
        asm.reg0().add_assign(Reg2);
        asm.reg0().store_mem_imm(ADDR_N2_X);
        // write n1y, n2y to memory
        asm.reg0().load_mem_imm(ADDR_PLAYER_Y as u8);
        asm.reg0().add_assign(Reg3);
        asm.reg0().store_mem_imm(ADDR_N1_Y);
        asm.reg0().add_assign(Reg3);
        asm.reg0().store_mem_imm(ADDR_N2_Y);

        // load n2 to reg3
        asm.reg0().load_mem_imm(ADDR_N2_X);
        asm.reg1().assign_from(Reg0);
        asm.reg0().load_mem_imm(ADDR_N2_Y);
        asm.reg2().assign_from(Reg0);
        read_map_tile(asm); // mem page changed
        asm.reg3().assign_from(Reg0);

        asm.reg0().load_imm(PAGE_GAME as u8);
        asm.reg0().set_mem_page();
        // load n1 to reg2
        asm.reg0().load_mem_imm(ADDR_N1_X);
        asm.reg1().assign_from(Reg0);
        asm.reg0().load_mem_imm(ADDR_N1_Y);
        asm.reg2().assign_from(Reg0);
        read_map_tile(asm); // mem page changed
        asm.reg2().assign_from(Reg0);

        asm.reg0().load_imm(PAGE_GAME as u8);
        asm.reg0().set_mem_page();

        asm.reg0().assign_from(Reg2);
        asm.reg0().store_mem_imm(ADDR_N1_TILE);
        asm.reg0().assign_from(Reg3);
        asm.reg0().store_mem_imm(ADDR_N2_TILE);
    }

    /// input: reg2: n1, reg3: n2, mem_page: PAGE_GAME
    /// output: nothing, map tiles already modified
    fn push_and_move(asm: &mut Assembler) {
        asm.comment("push_and_move".to_string());

        // core logic:
        // push = !N2_BOX && N2_GROUND && N1_BOX
        // if push -> N2 |= BOX, N1 &= ~BOX
        // move = N1_GROUND && (!N1_BOX || push)
        // if move -> player xy = N1, P &= ~PLAYER, N1 |= PLAYER

        // translated logic:

        // push_bit = 0
        // if N2_GROUND(0000 or 1000) > 0 {
        //     push_bit(0000 or 0010) = ~N2_BOX(1111 or 1101) & N1_BOX(0000 or 0010)
        // }
        // N2_TILE |= push_bit
        // N1_TILE &= ~push_bit

        asm.reg1().xor_assign(Reg1); // push_bit = 0

        asm.reg0().load_imm(TILE_BOX);
        asm.reg0().and_assign(Reg2);
        asm.reg0().store_mem_imm(ADDR_N1_BOX);
        asm.reg0().load_imm(TILE_GROUND);
        asm.reg0().and_assign(Reg2);
        asm.reg0().store_mem_imm(ADDR_N1_GROUND);
        asm.reg0().load_imm(TILE_BOX);
        asm.reg0().and_assign(Reg3);
        asm.reg0().store_mem_imm(ADDR_N2_BOX);
        asm.reg0().load_imm(TILE_GROUND);
        asm.reg0().and_assign(Reg3);
        asm.comment("test and push".to_string());
        asm.reg0().store_mem_imm(ADDR_N2_GROUND);

        asm.if_is_8_to_15(
            // N2_GROUND = 8
            |asm| {
                asm.reg0().load_mem_imm(ADDR_N2_BOX);
                asm.reg0().inv(); // ~N2_BOX
                asm.reg1().assign_from(Reg0);
            },
            |_| {},
        );
        asm.reg0().load_mem_imm(ADDR_N1_BOX);
        asm.reg1().and_assign(Reg0); // reg1 saves push_bit
        asm.reg3().assign_from(Reg0); // reg3 saves N1_BOX
        asm.reg2().assign_from(Reg1);
        asm.reg2().inv(); // reg2 saves ~push_bit
        asm.reg0().load_mem_imm(ADDR_N2_TILE);
        asm.reg0().or_assign(Reg1); // N2_TILE |= push_bit
        asm.reg0().store_mem_imm(ADDR_N2_TILE);
        asm.reg0().load_mem_imm(ADDR_N1_TILE);
        asm.reg0().and_assign(Reg2); // N1_TILE &= ~push_bit
        asm.reg0().store_mem_imm(ADDR_N1_TILE);

        // move_bit(1000 or 0000) = (~N1_BOX(1111 or 1101) | push_bit(0000 or 0010)) * 4 & N1_GROUND
        // if move_bit > 0 {
        //     N1 |= PLAYER
        //     P &= ~PLAYER (read, and, write)
        //     Player XY = N1 XY
        // }

        asm.comment("test and move".to_string());
        // reg0 N1 tile, reg1 push bit, reg3 N1_BOX
        asm.reg3().inv(); // ~N1_BOX
        asm.reg1().or_assign(Reg3);
        asm.reg1().add_assign(Reg1);
        asm.reg1().add_assign(Reg1); // reg1 = 1000 or 0000
        asm.reg0().load_mem_imm(ADDR_N1_GROUND);
        asm.reg1().and_assign(Reg0); // move_bit
        asm.if_is_zero(
            |asm| {
                asm.reg0().load_imm(0);
            },
            |asm| {
                asm.reg0().load_imm(TILE_PLAYER);
            },
        );
        asm.reg3().assign_from(Reg0);
        asm.reg0().load_mem_imm(ADDR_N1_TILE);
        asm.comment("    N1_TILE |= PLAYER".to_string());
        asm.reg0().or_assign(Reg3);
        asm.reg0().store_mem_imm(ADDR_N1_TILE);
        // read/write player tile
        asm.comment("    read/write player tile".to_string());
        asm.reg0().load_mem_imm(ADDR_PLAYER_X as u8);
        asm.reg1().assign_from(Reg0);
        asm.reg0().load_mem_imm(ADDR_PLAYER_Y as u8);
        asm.reg2().assign_from(Reg0);
        read_map_tile(asm); // mem page changed
        asm.reg3().assign_from(Reg3); // reset flag
        asm.if_is_zero(
            |_| {},
            |asm| {
                asm.reg3().inv();
                asm.reg0().and_assign(Reg3);
                asm.reg0().store_mem_reg();
            },
        );
        // write player xy
        asm.comment("    write player xy".to_string());
        asm.reg0().load_imm(PAGE_GAME as u8);
        asm.reg0().set_mem_page();
        asm.reg3().assign_from(Reg3);
        asm.if_is_zero(
            |_| {},
            |asm| {
                asm.reg0().load_mem_imm(ADDR_N1_X);
                asm.reg0().store_mem_imm(ADDR_PLAYER_X as u8);
                asm.reg0().load_mem_imm(ADDR_N1_Y);
                asm.reg0().store_mem_imm(ADDR_PLAYER_Y as u8);
            },
        );

        fn write_tile_to_mem(asm: &mut Assembler, addr_x: u8, addr_y: u8, addr_tile: u8) {
            asm.reg0().load_imm(PAGE_GAME as u8);
            asm.reg0().set_mem_page();
            asm.reg0().load_mem_imm(addr_x);
            asm.reg1().assign_from(Reg0);
            asm.reg0().load_mem_imm(addr_y);
            asm.reg2().assign_from(Reg0);
            asm.reg0().load_mem_imm(addr_tile);
            asm.reg3().assign_from(Reg0);

            asm.reg0().load_imm(PAGE_MAP as u8);
            asm.reg0().add_assign(Reg2);
            asm.reg0().set_mem_page();
            asm.reg0().assign_from(Reg3);
            asm.reg0().store_mem_reg(); // map[y][x] = reg0
        }
        // write n1 n2 tile to mem
        write_tile_to_mem(asm, ADDR_N1_X, ADDR_N1_Y, ADDR_N1_TILE);
        write_tile_to_mem(asm, ADDR_N2_X, ADDR_N2_Y, ADDR_N2_TILE);
    }

    read_input(asm);
    // reg1: any input

    // if no input => call render
    asm.reg1().assign_from(Reg1);
    asm.if_is_zero(|asm| asm.jmp_long("render"), |_| {});

    read_n1_n2(asm);

    push_and_move(asm);

    asm.jmp_long("render");
}

/// read map tiles, call graphics set pixel
/// assuming graphics on bus_addr1 and frame initialized
fn render(asm: &mut Assembler) {
    asm.reg0().xor_assign(Reg0);
    asm.reg1().xor_assign(Reg1);
    asm.comment("reset graphics cursor".to_string());
    asm.bus1(DeviceGraphicsV1Opcode::SetCursor as u8);

    // use reg2 as super heavy target checker, if any target => set high bits
    asm.reg0().load_imm(0b0011);
    asm.reg2().assign_from(Reg0);

    asm.comment("reg0: color, reg1: addr, reg3: page".to_string());
    asm.reg0().load_imm(PAGE_MAP as u8);
    asm.reg3().assign_from(Reg0);

    let page_loop = asm.reg0().assign_from(Reg3);
    asm.reg0().set_mem_page();
    asm.reg1().xor_assign(Reg1);

    let inner_loop = asm.reg0().load_mem_reg(); // reg0 = mem[page][reg1]
    asm.bus1(DeviceGraphicsV1Opcode::SetColorNext as u8);

    assert_eq!(TILE_BOX | TILE_TARGET, 0b11);
    asm.reg0().dec(); // (!box & target) tile becomes 0bXX00

    // mid point
    let skip_jmp_back = asm.jmp_forward();
    let inner_loop_mid = asm.jmp_back(inner_loop); // out of range, insert a mid point
    let page_loop_mid = asm.jmp_back(page_loop); // out of range, insert a mid point
    asm.resolve_jmp(skip_jmp_back);

    asm.reg0().and_assign(Reg2); // (!box & target) tile becomes 0b0000
    asm.if_is_zero_no_else(|asm| {
        // if zero => taret found => set flag to reg2
        // asm.reg0().load_imm(0b1111);
        // asm.reg2().assign_from(Reg0);

        // hack! Reg3 is [8, 16), high bits always on
        asm.reg2().or_assign(Reg3);
    });

    asm.reg1().inc();
    asm.jg_back(inner_loop_mid);

    asm.reg3().inc();
    asm.jne_back(page_loop_mid); // jmp to page loop if reg3 != 0 (overflow)

    asm.comment("check any '!box & target' tile".to_string());
    asm.reg2().assign_from(Reg2);
    // if == 3 => all target are satisfied
    asm.if_is_1_to_7(
        |asm| {
            asm.reg0().load_imm(PAGE_GAME as u8);
            asm.reg0().set_mem_page();
            asm.reg0().load_imm(GameState::Win as u8);
            asm.reg0().store_mem_imm(ADDR_GAME_STATE as u8);
        },
        |_| {},
    );

    asm.comment("finish frame".to_string());
    asm.bus1(DeviceGraphicsV1Opcode::SendFrameVsync as u8);

    asm.jmp_long("game_loop");
}
