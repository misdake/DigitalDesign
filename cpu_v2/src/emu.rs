struct EmuState {
    inst: Box<[u16; 65536]>,
    reg: [u16; 16],
    mem: Box<[u16; 65536]>,

    pc: u16,
    stack_ptr: u16,
    flag_s: u8,
    flag_z: u8,
}
