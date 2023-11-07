use crate::cpu_v1::assembler::{Assembler, RegisterCommon, RegisterSpecial};
use crate::cpu_v1::devices::*;

const MAP_WIDTH: usize = 8;
const MAP_HEIGHT: usize = 8;
const TARGET_MAX: usize = 8;

const ADDR_PLAYER_X: usize = 0;
const ADDR_PLAYER_Y: usize = 1;
const ADDR_TARGET_COUNT: usize = 2;
const ADDR_PALETTE: usize = 10 * 16;
const ADDR_TARGET_LIST: usize = 11 * 16; // max 8x2
const ADDR_MAP: usize = 12 * 16;

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
    map: [u8; MAP_WIDTH * MAP_HEIGHT],
}
impl GameMap {
    fn parse(tiles: [&'static str; MAP_HEIGHT]) -> Self {
        let mut map = GameMap {
            start: (0, 0),
            target_list: vec![],
            map: [0; MAP_WIDTH * MAP_HEIGHT],
        };

        for y in 0..MAP_HEIGHT {
            assert_eq!(tiles[y].len(), MAP_WIDTH);
            for (x, c) in tiles[y].chars().enumerate() {
                let i = parse_tile(c);
                map.map[y * MAP_HEIGHT + x] = i;
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
            r[ADDR_PALETTE + i] = PALETTE[i] as u8;
        }
        for (i, (x, y)) in self.target_list.iter().enumerate() {
            r[ADDR_TARGET_LIST + i * 2] = *x as u8;
            r[ADDR_TARGET_LIST + i * 2 + 1] = *y as u8;
        }
        for i in 0..(MAP_WIDTH * MAP_HEIGHT) {
            r[ADDR_MAP + i] = self.map[i];
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

    set_rom_content(&rom);

    let mut asm = Assembler::new();

    init(&mut asm);

    //TODO gameloop

    //TODO read input
    //TODO gameplay

    //TODO render
}

/// set devices, set gamepad mode, set palette
fn init(asm: &mut Assembler) {
    use crate::cpu_v1::isa::Instruction::*;
    use crate::cpu_v1::isa::RegisterIndex::*;

    /// assuming rom cursor=0
    fn copy_rom(asm: &mut Assembler) {
        [
            load_imm(0),
            load_imm(DeviceType::Rom as u8),
            set_bus_addr0(()),
            mov((Reg0, Reg3)), // write page to reg3
            mov((Reg0, Reg1)), // reg1 <- 0
            // page loop
            mov((Reg3, Reg0)), // reg0 <- reg3
            set_mem_page(()),  // set page <- reg0
            inc(Reg3),         // reg3++
            // inner loop
            bus0(DeviceRomOpcode::ReadNext as u8), // reg0 <- rom[cursor++]
            store_mem(0),                          // mem[page][reg1] <- reg0
            inc(Reg1),                             // reg1++
            jne_offset(16 - 3),                    // jmp to inner loop if reg1 != 0 (overflow)
            // inner loop finish
            mov((Reg3, Reg3)),  // set flags of reg3
            jne_offset(16 - 8), // jmp to page loop if reg3 != 0 (overflow)
        ]
        .into_iter()
        .for_each(|inst| {
            asm.inst(inst);
        });
    }

    /// assuming graphics on bus_addr1
    fn load_palette(asm: &mut Assembler) {
        assert_eq!(ADDR_PALETTE & 0b1111, 0); // multiple of 16
        asm.reg0().load_imm((ADDR_PALETTE / 16) as u8);
        asm.inst(set_mem_page(())); // set page <- reg0
        asm.reg0().load_imm(0);
        asm.reg1().assign_from(Reg0);
        let loop_start = asm.inst(load_mem(0)); // reg0 = mem[page][reg1]
        asm.inst(bus1(DeviceGraphicsV1Opcode::SetPalette as u8));
        asm.reg1().inc();
        asm.jne_offset(loop_start);
    }

    // copy all from rom
    copy_rom(asm);

    // setup devices, gamepad on addr0, graphics on addr1
    asm.inst(load_imm(DeviceType::Gamepad as u8));
    asm.inst(set_bus_addr0(()));
    asm.inst(load_imm(DeviceType::GraphicsV1 as u8));
    asm.inst(set_bus_addr1(()));
    asm.inst(load_imm(ButtonQueryMode::Press as u8));
    asm.inst(bus0(DeviceGamepadOpcode::SetButtonQueryMode as u8));

    // load palette to graphics
    load_palette(asm);
}
