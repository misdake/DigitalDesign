//! Production adjacent-tap Common72 / Member76. Generic Membership is unchanged.
use super::{compact_transport::Member, runtime_membership::Input};
pub(super) struct Pipeline {
    short: bool,
    common: Option<u128>,
    member: Option<Member>,
    tail: [Option<Member>; 5],
    fault: bool,
}
impl Pipeline {
    pub(super) fn with_short_alignment(short: bool) -> Self {
        Self {
            short,
            common: None,
            member: None,
            tail: [None; 5],
            fault: false,
        }
    }
    pub(super) fn output(&self) -> Option<Member> {
        if self.short {
            self.member
        } else {
            self.tail[4]
        }
    }
    pub(super) fn inflight(&self) -> usize {
        usize::from(self.common.is_some())
            + usize::from(self.member.is_some())
            + self.tail.iter().filter(|v| v.is_some()).count()
    }
    pub(super) fn tick(
        &mut self,
        ce: bool,
        input: Option<Input>,
    ) -> Result<Option<Member>, String> {
        if self.fault {
            return Err("compact membership terminal".into());
        }
        if !ce {
            return Ok(None);
        }
        let captured = input
            .map(|i| {
                let i = i.validate()?;
                let mask = crate::texture::emu::compact::side_mask(i.level);
                if i.level > 10
                    || i.coordinates[1] != (i.coordinates[0] + 1) & mask
                    || i.coordinates[3] != (i.coordinates[2] + 1) & mask
                    || i.coordinates.iter().any(|v| *v > mask)
                {
                    return Err("compact membership adjacent domain".into());
                }
                let mut word = 0;
                for (n, w) in i.weights.into_iter().enumerate() {
                    word |= u128::from(w) << (n * 9);
                }
                Ok(word
                    | u128::from(i.coordinates[0] >> 3) << 36
                    | u128::from(i.coordinates[2] >> 3) << 43
                    | u128::from(i.coordinates[0] & 7) << 50
                    | u128::from(i.coordinates[2] & 7) << 53
                    | u128::from(i.slot) << 56
                    | u128::from(i.level) << 60
                    | u128::from(i.key / 4) << 64
                    | u128::from(i.key % 4) << 68
                    | u128::from(i.fine) << 70
                    | u128::from(!i.fine || i.last_fine) << 71)
            })
            .transpose();
        let captured = match captured {
            Ok(v) => v,
            Err(e) => {
                self.fault = true;
                return Err(e);
            }
        };
        let old = self.output();
        let member = self.common.map(|w| {
            let sx = (w >> 60 & 15) <= 3 || (w >> 50 & 7) != 7;
            let sy = (w >> 60 & 15) <= 3 || (w >> 53 & 7) != 7;
            let n = std::array::from_fn::<_, 4, _>(|i| w >> (i * 9) & 511 != 0);
            let e = [
                n[0],
                n[1] && !(n[0] && sx),
                n[2] && !((n[0] && sy) || (n[1] && sx && sy)),
                n[3] && !((n[0] && sx && sy) || (n[1] && sy) || (n[2] && sx)),
            ];
            let emit = e
                .into_iter()
                .enumerate()
                .fold(0, |v, (i, e)| v | (u128::from(e) << i));
            Member((w & ((1 << 36) - 1)) | (emit << 36) | ((w >> 36) << 40))
        });
        if !self.short {
            for i in (1..5).rev() {
                self.tail[i] = self.tail[i - 1];
            }
            self.tail[0] = self.member;
        }
        self.member = member;
        self.common = captured;
        Ok(old)
    }
    #[cfg(test)]
    pub(super) fn banks(&self) -> [Option<u128>; 7] {
        if self.short {
            [
                self.common,
                None,
                None,
                None,
                self.member.map(Member::bits),
                None,
                None,
            ]
        } else {
            [
                self.common,
                self.member.map(Member::bits),
                self.tail[0].map(Member::bits),
                self.tail[1].map(Member::bits),
                self.tail[2].map(Member::bits),
                self.tail[3].map(Member::bits),
                self.tail[4].map(Member::bits),
            ]
        }
    }
}
