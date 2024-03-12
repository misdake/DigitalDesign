use proc_macro::{TokenStream, TokenTree};

// define_isa!{
//     Isa
//     (add, 0b0000, RRR, "r{0} = r{1} + r{2}"),
// }

// pub enum Isa {
//     ...
// }
// impl Isa {
//     pub fn encode...
//     pub fn parse...
// }
// impl Display for Isa {
//     fn fmt...
// }
// pub fn add(reg2: Reg, reg1: Reg, reg0: Reg) -> Isa { Isa::add(reg2, reg1, reg0) }

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

#[derive(Debug)]
struct IsaDesc {
    isa_name: String,
    insts: Vec<InstDesc>,
}
impl IsaDesc {
    pub fn generate(&self) -> String {
        let name = self.isa_name.as_str();
        let enum_decl = self.enum_decl();
        let encode = self.encode();
        let parse = self.parse();
        let display = self.display();
        let constructor = self.constructor();
        format!(
            "{enum_decl}\n#[rustfmt::skip]\nimpl {name} {{\n{encode}\n{parse}\n}}\n{display}\n{constructor}",
        )
    }

    fn enum_decl(&self) -> String {
        //pub enum Isa {
        //    add(Reg, Reg, Reg),
        //}
        format!(
            "#[allow(non_camel_case_types)]\n#[derive(Copy, Clone)]\npub enum {} {{\n{}\n}}",
            self.isa_name.as_str(),
            self.insts
                .iter()
                .map(|i| i.enum_item())
                .collect::<Vec<_>>()
                .join(",\n    ")
        )
    }
    fn encode(&self) -> String {
        //    pub fn encode(&self) -> u16 {
        //        use Isa::*;
        //        match self {
        //            add(reg2, reg1, reg0) => { (0b0000 << 12) | ((*reg2 as u16) << 8) | ((*reg1 as u16) << 4) | ((*reg0 as u16) << 0) }
        //        }
        //    }
        format!(
            "    #[allow(clippy::eq_op)]\n    #[allow(clippy::identity_op)]\n    pub fn encode(&self) -> u16 {{\n        use {}::*;\n        match self {{\n            {}\n        }}\n    }}",
            self.isa_name.as_str(),
            self.insts
                .iter()
                .map(|i| i.encode_item())
                .collect::<Vec<_>>()
                .join("\n            ")
        )
    }
    fn parse(&self) -> String {
        //    pub fn parse(inst: InstBinaryType) -> Self {
        //        use Isa::*;
        //        if part3(inst) == 0b0000 { return add(part2(inst), part1(inst), part0(inst)); }
        //        unreachable!()
        //    }
        format!(
            "    pub fn parse(inst: InstBinaryType) -> Self {{\n        use {}::*;\n        {}\n        unreachable!()\n    }}",
            self.isa_name.as_str(),
            self.insts
                .iter()
                .map(|i| i.parse())
                .collect::<Vec<_>>()
                .join("\n        ")
        )
    }
    fn display(&self) -> String {
        //impl std::fmt::Display for Isa {
        //    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        //        use Isa::*;
        //        match self {
        //            add(reg2, reg1, reg0) => { write!(f, "r{0} = r{1} + r{2}", reg0, reg1, reg2) }
        //        }
        //    }
        //}
        format!(
            "impl std::fmt::Display for {} {{\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n        use {}::*;\n        match self {{\n            {}\n        }}\n    }}\n}}",
            self.isa_name.as_str(),
            self.isa_name.as_str(),
            self.insts
                .iter()
                .map(|i| i.display())
                .collect::<Vec<_>>()
                .join("\n            ")
        )
    }
    fn constructor(&self) -> String {
        //pub fn add(reg2: Reg, reg1: Reg, reg0: Reg) -> Isa { Isa::add(reg2, reg1, reg0) }
        let name = self.isa_name.as_str();
        self.insts
            .iter()
            .map(|inst| inst.constructor(name))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

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
    /// e.g. add(reg2, reg1, reg0)
    fn match_item(&self) -> String {
        format!(
            "{}({})",
            self.inst_name,
            self.params(|p, pos| p.param_name(pos), ", ")
        )
    }
    /// e.g. add(reg2, reg1, reg0) => { (0b0000 << 12) | (reg2 << 8) | (reg1 << 4) | (reg0 << 0) }
    fn encode_item(&self) -> String {
        let match_item = self.match_item();
        let parts = self.params(|p, pos| p.value(pos), " | ");
        format!("{match_item} => {{ {parts} }}")
    }
    /// e.g. if part3(inst) == 0b0000 { return add(part2(inst), part1(inst), part0(inst)); }
    fn parse(&self) -> String {
        let check = self.params(|p, pos| p.check_op(pos), " && ");
        let parts = self.params(|p, pos| p.parse_part(pos), ", ");
        let inst_name = &self.inst_name;
        format!("if {check} {{ return {inst_name}({parts}); }}")
    }
    /// e.g. add(reg2, reg1, reg0) => { write!(f, "r{0} = r{1} + r{2}", reg0, reg1, reg2) }
    fn display(&self) -> String {
        let match_item = self.match_item();
        let reverse = self.params_rev(|p, pos| p.param_name(pos), ", ");
        format!(
            "{match_item} => {{ write!(f, {}, {reverse}) }}",
            self.format
        )
    }
    /// e.g. pub fn add(reg2: Reg, reg1: Reg, reg0: Reg) -> Isa { Isa::add(reg2, reg1, reg0) }
    fn constructor(&self, isa_name: &str) -> String {
        let params = self.params(|p, pos| p.param_decl(pos), ", ");
        let names = self.params(|p, pos| p.param_name(pos), ", ");
        let inst_name = &self.inst_name;
        format!("pub fn {inst_name}({params}) -> {isa_name} {{ {isa_name}::{inst_name}({names}) }}")
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

    fn params_rev<F: Fn(EncodingPart, usize) -> Option<String>>(
        &self,
        f: F,
        join: &'static str,
    ) -> String {
        [
            f(self.type0, 0),
            f(self.type1, 1),
            f(self.type2, 2),
            f(self.type3, 3),
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
    F,     // jmp flags
    X,     // nothing
}
impl EncodingPart {
    fn parse_macro(ty: char, op: u8) -> Self {
        match ty {
            'O' => EncodingPart::O(op),
            'R' => EncodingPart::R,
            'I' => EncodingPart::I,
            'F' => EncodingPart::F,
            'X' => EncodingPart::X,
            _ => unreachable!(),
        }
    }

    fn parse_part(self, pos: usize) -> Option<String> {
        match self {
            EncodingPart::O(_) => None,
            EncodingPart::R => Some(format!("part{pos}(inst)")),
            EncodingPart::I => Some(format!("part{pos}(inst)")),
            EncodingPart::F => Some(format!("part{pos}(inst)")),
            EncodingPart::X => None,
        }
    }
    fn param_name(self, pos: usize) -> Option<String> {
        match self {
            EncodingPart::O(_) => None,
            EncodingPart::R => Some(format!("reg{pos}")),
            EncodingPart::I => Some(format!("imm{pos}")),
            EncodingPart::F => Some("flags".to_string()),
            EncodingPart::X => None,
        }
    }
    fn param_type(self, _: usize) -> Option<String> {
        match self {
            EncodingPart::O(_) => None,
            EncodingPart::R => Some("Reg".to_string()),
            EncodingPart::I => Some("Imm4".to_string()),
            EncodingPart::F => Some("Flag4".to_string()),
            EncodingPart::X => None,
        }
    }
    fn param_decl(self, pos: usize) -> Option<String> {
        match self {
            EncodingPart::O(_) => None,
            EncodingPart::R => Some(format!("reg{pos}: Reg")),
            EncodingPart::I => Some(format!("imm{pos}: Imm4")),
            EncodingPart::F => Some("flags: Flag4".to_string()),
            EncodingPart::X => None,
        }
    }
    fn check_op(self, pos: usize) -> Option<String> {
        if let EncodingPart::O(op) = self {
            Some(format!("part{pos}(inst) == 0b{op:04b}"))
        } else {
            None
        }
    }
    fn value(self, pos: usize) -> Option<String> {
        match self {
            EncodingPart::O(op) => Some(format!("(0b{op:04b} << {})", pos * 4)),
            EncodingPart::R => Some(format!("((*reg{pos} as u16) << {})", pos * 4)),
            EncodingPart::I => Some(format!("((*imm{pos} as u16) << {})", pos * 4)),
            EncodingPart::F => Some(format!("((*flags as u16) << {})", pos * 4)),
            EncodingPart::X => None,
        }
    }
}

fn parse_isa_desc(insts: TokenStream) -> IsaDesc {
    let mut iter = insts.into_iter();

    let isa_name = iter.next();

    let inst_desc = iter
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

                let op_count = types.iter().filter(|&&ty| ty == 'O').count();
                // count = 1 -> offset =  0, 0, 0, 0
                // count = 2 -> offset =  4, 0, 0, 0
                // count = 3 -> offset =  8, 4, 0, 0
                // count = 4 -> offset = 12, 8, 4, 0
                let op_offset3 = op_count.saturating_sub(1) * 4;
                let op_offset2 = op_count.saturating_sub(2) * 4;
                let op_offset1 = op_count.saturating_sub(3) * 4;
                let op_offset0 = op_count.saturating_sub(4) * 4;
                let op3 = ((op >> op_offset3) & 0b1111) as u8;
                let op2 = ((op >> op_offset2) & 0b1111) as u8;
                let op1 = ((op >> op_offset1) & 0b1111) as u8;
                let op0 = ((op >> op_offset0) & 0b1111) as u8;

                InstDesc {
                    inst_name: name,
                    type3: EncodingPart::parse_macro(types[0], op3),
                    type2: EncodingPart::parse_macro(types[1], op2),
                    type1: EncodingPart::parse_macro(types[2], op1),
                    type0: EncodingPart::parse_macro(types[3], op0),
                    format: to_string,
                }
            } else {
                unreachable!("each isa inst should be token group: (name op encoding to_string)")
            }
        })
        .collect::<Vec<_>>();

    IsaDesc {
        isa_name: isa_name.unwrap().to_string(),
        insts: inst_desc,
    }
}

#[proc_macro]
pub fn define_isa(insts: TokenStream) -> TokenStream {
    let isa_desc = parse_isa_desc(insts);
    // println!("{isa_desc:?}");

    let generated = isa_desc.generate();
    // println!("{}", generated);

    generated.parse().unwrap()
}
