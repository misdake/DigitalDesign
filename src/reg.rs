use crate::{cycle, mux2, Wire};

pub fn reg(input: Wire) -> Wire {
    cycle(|_| input)
}

pub fn flipflop(data: Wire, write_enabled: Wire) -> Wire {
    cycle(|saved| mux2(saved, data, write_enabled))
}
