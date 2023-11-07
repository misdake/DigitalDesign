use crate::cpu_v1::assembler::{Assembler, RegisterCommon, RegisterSpecial};
use crate::cpu_v1::cpu_v1_build_mix;
use crate::cpu_v1::devices::*;
use crate::{clock_tick, execute_gates};

const MAP_SIZE: usize = 8;
const TARGET_MAX: usize = 8;

const ADDR_PLAYER_X: usize = 0;
const ADDR_PLAYER_Y: usize = 1;
const ADDR_TARGET_COUNT: usize = 2;
const PAGE_PALETTE: usize = 10;
const PAGE_TARGET_LIST: usize = 11; // max 8 pairs of xy
const PAGE_MAP: usize = 12; // [12, 16)

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
        '.' => 0b0000, // ground
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

        for i in 0..16 {
            r[PAGE_PALETTE * 16 + i] = PALETTE[i] as u8;
        }
        for (i, (x, y)) in self.target_list.iter().enumerate() {
            r[PAGE_TARGET_LIST * 16 + i * 2] = *x as u8;
            r[PAGE_TARGET_LIST * 16 + i * 2 + 1] = *y as u8;
        }
        for i in 0..(MAP_SIZE * MAP_SIZE) {
            r[PAGE_MAP * 16 + i] = self.map[i];
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

    let mut asm = Assembler::new();

    asm.func("init", 0, |asm, _| init(asm));

    //TODO gameloop

    //TODO read input
    //TODO gameplay

    asm.func("render", 14, |asm, _| render(asm));

    start(asm);
}

fn start(asm: Assembler) {
    println!("asm:\n{}\n", asm.to_pretty_string());
    let inst = asm.finish();

    let (state, _state_ref) = cpu_v1_build_mix(inst);

    loop {
        let pc = state.pc.out.get_u8();
        if pc as usize >= inst.len() {
            break;
        }
        // let inst_desc = inst[pc as usize];
        // println!("pc {:08b}: inst {}", pc, inst_desc.to_string());

        execute_gates();

        clock_tick();
    }
}

/// set devices, set gamepad mode, set palette
fn init(asm: &mut Assembler) {
    use crate::cpu_v1::isa::Instruction::*;
    use crate::cpu_v1::isa::RegisterIndex::*;

    /// assuming rom cursor=0
    fn copy_rom(asm: &mut Assembler) {
        asm.reg0().load_imm(DeviceType::Rom as u8);
        asm.inst(set_bus_addr0(()));
        asm.reg0().load_imm(0);
        asm.reg3().assign_from(Reg0); // reg3 <- 0 (reg3 saves page index)
        asm.reg1().assign_from(Reg0); // reg1 <- 0 (addr in page)

        asm.comment("start copy rom to mem".to_string());
        let page_loop = asm.reg0().assign_from(Reg3);
        asm.inst(set_mem_page(()));
        asm.reg3().inc();

        let inner_loop = asm.inst(bus0(DeviceRomOpcode::ReadNext as u8)); // reg0 <- rom[cursor++]
        asm.inst(store_mem(0)); // mem[page][reg1] <- reg0
        asm.reg1().inc();
        asm.jne_offset(inner_loop); // jmp to inner loop if reg1 != 0 (overflow)

        asm.reg3().assign_from(Reg3); // set flags of reg3
        asm.jne_offset(page_loop); // jmp to page loop if reg3 != 0 (overflow)
    }

    /// assuming graphics on bus_addr1
    fn load_palette(asm: &mut Assembler) {
        asm.comment("load palette".to_string());
        asm.reg0().load_imm(PAGE_PALETTE as u8);
        asm.inst(set_mem_page(())); // set page <- reg0
        asm.reg0().load_imm(0);
        asm.reg1().assign_from(Reg0);
        // do { load_mem; set_palette; inc(reg1); } while reg1!=0 (overflow)
        let loop_start = asm.inst(load_mem(0)); // reg0 = mem[page][reg1]
        asm.inst(bus1(DeviceGraphicsV1Opcode::SetPalette as u8));
        asm.reg1().inc();
        asm.jne_offset(loop_start);
    }

    // copy all from rom
    copy_rom(asm);

    // setup devices, gamepad on addr0, graphics on addr1
    asm.comment("init gamepad and graphics".to_string());
    asm.inst(load_imm(DeviceType::Gamepad as u8));
    asm.inst(set_bus_addr0(()));
    asm.inst(load_imm(DeviceType::GraphicsV1 as u8));
    asm.inst(set_bus_addr1(()));

    asm.reg0().load_imm(MAP_SIZE as u8);
    asm.reg1().assign_from(Reg0);
    asm.inst(bus1(DeviceGraphicsV1Opcode::Resize as u8));

    asm.inst(load_imm(ButtonQueryMode::Press as u8));
    asm.inst(bus0(DeviceGamepadOpcode::SetButtonQueryMode as u8));

    // load palette to graphics
    load_palette(asm);
}

/// read map tiles, call graphics set pixel
/// assuming graphics on bus_addr1 and frame initialized
fn render(asm: &mut Assembler) {
    use crate::cpu_v1::isa::Instruction::*;
    use crate::cpu_v1::isa::RegisterIndex::*;

    asm.reg0().load_imm(0);
    asm.reg1().assign_from(Reg0);
    asm.comment("reset graphics cursor".to_string());
    asm.inst(bus1(DeviceGraphicsV1Opcode::SetCursor as u8));

    asm.comment("reg0: color, reg1: addr, reg3: page".to_string());
    asm.reg0().load_imm(PAGE_MAP as u8);
    asm.reg3().assign_from(Reg0);

    let page_loop = asm.reg0().assign_from(Reg3);
    asm.inst(set_mem_page(()));
    asm.reg3().inc();

    let inner_loop = asm.inst(load_mem(0)); // reg0 = mem[page][reg1]
    asm.inst(bus1(DeviceGraphicsV1Opcode::SetColorNext as u8));
    asm.reg1().inc();
    asm.jne_offset(inner_loop);

    asm.reg3().assign_from(Reg3); // set flags of reg3
    asm.jne_offset(page_loop); // jmp to page loop if reg3 != 0 (overflow)

    asm.comment("finish frame".to_string());
    asm.inst(bus0(DeviceGamepadOpcode::NextFrame as u8));
    asm.inst(bus1(DeviceGraphicsV1Opcode::PresentFrame as u8));
    asm.inst(bus1(DeviceGraphicsV1Opcode::WaitNextFrame as u8));

    asm.jmp_long("render");
}
