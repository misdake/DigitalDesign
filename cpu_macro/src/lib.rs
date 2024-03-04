use proc_macro::{Group, TokenStream, TokenTree};
use syn::parse;

#[proc_macro]
pub fn make_answer(item: TokenStream) -> TokenStream {
    for t in item {
        println!("{t}");
    }

    "fn answer() -> u32 { 42 }".parse().unwrap()
}

// pub enum IsaX {
//     Add(Reg, Reg, Reg), // reg2, reg1, reg0 TODO InstDesc::enum_item
// }
// impl IsaX {
//     pub fn encode(&self) -> u16 {
//         match self {
//             IsaX::Add(reg2, reg1, reg0) => {
//                 part3(0b0000) | part2(reg2) | part1(reg1) | part0(reg0) TODO!
//             }
//         }
//     }
//     pub fn parse(inst: InstBinaryType) -> Self {
//         if part3(inst) == 0b0000 { return add(part2(inst), part1(inst), part0(inst)); } TODO InstDesc::parse
//
//         unreachable!()
//     }
// }
// impl Display for IsaX { TODO!
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         match self {
//             IsaX::Add(reg2, reg1, reg0) => {
//                 write!(f, "r{reg0} = r{reg1} + r{reg2}")
//             }
//         }
//     }
// }
// pub fn add(reg2: Reg, reg1: Reg, reg0: Reg) -> IsaX { TODO!
//     IsaX::Add(reg2, reg1, reg0)
// }

// define_isa!{
//     (add, 0b0000, RRR, "r{0} = r{1} + r{2}"),
// }

fn parse_u16(s: &str) -> Result<u16, std::num::ParseIntError> {
    if let Some(s) = s.strip_prefix("0x") {
        u16::from_str_radix(s, 16)
    } else if let Some(s) = s.strip_prefix("0o") {
        u16::from_str_radix(s, 8)
    } else if let Some(s) = s.strip_prefix("0b") {
        u16::from_str_radix(s, 2)
    } else {
        s.parse::<u16>()
    }
}

struct IsaDesc {
    isa_name: String,
    insts: Vec<InstDesc>,
}
// TODO isa fn

#[derive(Debug)]
struct InstDesc {
    inst_name: String,
    type3: EncodingPart, // highest
    type2: EncodingPart,
    type1: EncodingPart,
    type0: EncodingPart, // lowest
    format: String,
}
impl InstDesc {
    /// e.g. add(Reg, Reg, Reg)
    fn enum_item(&self) -> String {
        format!(
            "{}({})",
            self.inst_name,
            self.params(|p, pos| p.param_type(pos), ", ")
        )
    }
    /// e.g. if part3(inst) == 0b0000 { return add(part2(inst), part1(inst), part0(inst)); }
    fn parse(&self) -> String {
        let check = self.params(|p, pos| p.check_op(pos), " && ");
        let parts = self.params(|p, pos| p.parse_part(pos), ", ");
        let inst_name = &self.inst_name;
        format!("if {check} {{ return {inst_name}({parts}); }}")
    }

    fn params<F: Fn(EncodingPart, usize) -> Option<String>>(
        &self,
        f: F,
        join: &'static str,
    ) -> String {
        [
            f(self.type3, 3),
            f(self.type2, 2),
            f(self.type1, 1),
            f(self.type0, 0),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(join)
    }
}

#[derive(Debug, Copy, Clone)]
enum EncodingPart {
    O(u8), // opcode4
    R,     // reg
    I,     // imm4
    J,     // jmp flags
    X,     // nothing
}
impl EncodingPart {
    fn parse_macro(ty: char, op: u16, part_index: usize) -> Self {
        let op = (op >> (part_index * 4)) as u8;
        match ty {
            'O' => EncodingPart::O(op),
            'R' => EncodingPart::R,
            'I' => EncodingPart::I,
            'J' => EncodingPart::J,
            'X' => EncodingPart::X,
            _ => unreachable!(),
        }
    }

    fn parse_part(self, pos: usize) -> Option<String> {
        match self {
            EncodingPart::O(_) => None,
            EncodingPart::R => Some(format!("part{pos}(inst)")),
            EncodingPart::I => Some(format!("part{pos}(inst)")),
            EncodingPart::J => Some(format!("part{pos}(inst)")),
            EncodingPart::X => None,
        }
    }
    fn param_name(self, pos: usize) -> Option<String> {
        match self {
            EncodingPart::O(_) => None,
            EncodingPart::R => Some(format!("reg{pos}")),
            EncodingPart::I => Some("imm4".to_string()),
            EncodingPart::J => Some("jflags".to_string()),
            EncodingPart::X => None,
        }
    }
    fn param_type(self, _: usize) -> Option<String> {
        match self {
            EncodingPart::O(_) => None,
            EncodingPart::R => Some("Reg".to_string()),
            EncodingPart::I => Some("Imm4".to_string()),
            EncodingPart::J => Some("Flag4".to_string()),
            EncodingPart::X => None,
        }
    }
    fn param_decl(self, pos: usize) -> Option<String> {
        match self {
            EncodingPart::O(_) => None,
            EncodingPart::R => Some(format!("reg{pos}: Reg")),
            EncodingPart::I => Some("imm4: Imm4".to_string()),
            EncodingPart::J => Some("jflags: Flag4".to_string()),
            EncodingPart::X => None,
        }
    }
    fn check_op(self, pos: usize) -> Option<String> {
        if let EncodingPart::O(op) = self {
            Some(format!("part{pos} == 0b{op:04b}"))
        } else {
            None
        }
    }
}

fn parse_inst_descs(insts: TokenStream) -> Vec<InstDesc> {
    //TODO read isa name
    insts
        .into_iter()
        .map(|inst| {
            println!("{}", inst);
            if let TokenTree::Group(inst) = inst {
                let desc = inst.stream();
                let list = desc
                    .into_iter()
                    .map(|token| token.to_string())
                    .collect::<Vec<String>>();
                assert_eq!(
                    list.len(),
                    4,
                    "each isa inst should have 4 items: (name op encoding to_string)"
                );
                let name = list[0].clone();
                let op = parse_u16(list[1].as_str()).unwrap();
                let encoding = list[2].clone();
                let to_string = list[3].clone();

                assert_eq!(
                    encoding.len(),
                    4,
                    "encoding should consist of 4 parts, left to right: part3210"
                );
                let types = encoding.chars().collect::<Vec<_>>();

                InstDesc {
                    inst_name: name,
                    type3: EncodingPart::parse_macro(types[0], op, 3), // types is left to right
                    type2: EncodingPart::parse_macro(types[1], op, 2),
                    type1: EncodingPart::parse_macro(types[2], op, 1),
                    type0: EncodingPart::parse_macro(types[3], op, 0),
                    format: to_string,
                }
            } else {
                unreachable!("each isa inst should be token group: (name op encoding to_string)")
            }
        })
        .collect::<Vec<_>>()
}

#[proc_macro]
pub fn define_isa(insts: TokenStream) -> TokenStream {
    let inst_descs = parse_inst_descs(insts);

    for inst_desc in inst_descs {
        println!("{inst_desc:?}");
    }

    "fn answer() -> u32 { 42 }".parse().unwrap()
}
