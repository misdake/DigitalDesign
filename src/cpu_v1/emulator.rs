struct Emulator {
    inst: [u8; 256],

    pc: u8,

    mem: [u8; 16],
    reg: [u8; 4],

    flag_p: bool,
    flag_z: bool,
    flag_n: bool,
}
