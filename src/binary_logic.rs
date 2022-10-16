use crate::{nand, Wire};
use std::ops;

impl ops::Not for Wire {
    type Output = Wire;
    fn not(self) -> Self::Output {
        nand(self, self)
    }
}

impl ops::BitOr<Wire> for Wire {
    type Output = Wire;
    fn bitor(self, rhs: Wire) -> Self::Output {
        nand(!self, !rhs)
    }
}

impl ops::BitAnd<Wire> for Wire {
    type Output = Wire;
    fn bitand(self, rhs: Wire) -> Self::Output {
        !nand(self, rhs)
    }
}

impl ops::BitXor<Wire> for Wire {
    type Output = Wire;
    fn bitxor(self, rhs: Wire) -> Self::Output {
        let c = nand(self, rhs);
        nand(nand(self, c), nand(rhs, c))
    }
}
