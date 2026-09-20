//! FPU v2 special-function reference model: the hidden-BSRAM LUT contents and
//! the pure-integer RCP/RSQRT/SINCOS datapath (design `fpu-design-v2` section
//! 9.4, Stage 7a, revised freeze 2026-09-19).
//!
//! This module lives in the **architecture** layer and is the single source of
//! truth for the three LUTs and the special-function arithmetic. The hardware
//! layer depends downward on it: [`render_lut_init_region`] emits the
//! register-file Verilog BSRAM `initial` block from these tables and the
//! architecture [`crate::CpuV3Sim`] uses the same reference functions, so the
//! emulator and the RTL can never disagree through a copied table or formula.
//! The arrays are checked into the source as literals so that neither path
//! builds a table at run time. The literals were generated offline with the
//! f64 formulas below and are re-derived by a test so a transcription slip
//! cannot survive `cargo test`.
//!
//! ```text
//! RCP_LUT[i]        = round(1 / (1 + i/128)         * 2^16), i = 0..127
//! RSQRT_EVEN_LUT[i] = round(1 / sqrt(1 + i/128)     * 2^16), i = 0..127
//! RSQRT_ODD_LUT[i]  = round(1 / sqrt(2*(1 + i/128)) * 2^16), i = 0..127
//! SINCOS_LUT[i]     = round(sin(pi/2 * i/256)       * 2^16), i = 0..=256
//! ```
//!
//! The hidden region's two SDPB mirrors carry different halves, which doubles
//! the effective read-only capacity to 896 words. That split is a Verilog
//! concern; this module only supplies the plain tables:
//!
//! ```text
//! mirror A: RCP         @ 128..255, SINCOS    @ 256..511
//! mirror B: RSQRT even  @ 128..255, RSQRT odd @ 256..383
//! ```
//!
//! Hidden words are packed interpolation intervals rather than bare samples:
//! bits 16:0 hold `current` and bits 26:17 hold the signed 10-bit
//! `next-current` delta. This uses the otherwise-idle upper half of each
//! 32-bit BSRAM word, removes the second table read and runtime subtraction,
//! and still reproduces the reference interpolation bit for bit.
//!
//! The interpolation arithmetic is exactly the integer shape the RTL will use:
//! `delta = next - current` (signed), `current + ((delta * residue) >> shift)`
//! with an arithmetic shift. RCP and both RSQRT tables are 128-entry with a
//! 9-bit residue (`>> 9`); the 257-entry SINCOS table uses an 8-bit residue
//! (`>> 8`). `delta` fits signed 10 bits (see the packing test), so the product
//! is a plain 18x18 DSP-class signed multiply that cannot overflow `i32`.
//!
//! RSQRT normalizes `x = m * 2^e` with `m in [1,2)` exactly like RCP and picks
//! its table by the parity of `e`: even selects [`RSQRT_EVEN_LUT`]
//! (`1/sqrt(m)`), odd selects [`RSQRT_ODD_LUT`] (`1/sqrt(2m)`), and the result
//! rescales by `2^(-floor(e/2))`. The index and residue are plain bit slices of
//! the normalized magnitude, so the datapath has no division of any kind.
//!
//! This revised freeze drops the RSQRT Newton step and the shared multiplication
//! pipe entirely: the finer tables (error ~ h^2) meet the targets on their own,
//! and the 32x32-class range reduction is replaced by a decomposed two-term
//! constant that only forms 16x16-class partial products.
//!
//! The architectural emulator consumes the datapath, the hardware layer
//! consumes the packed tables, and the `fpu_lut_codegen` example regenerates
//! the register-file Verilog `initial` block from [`render_lut_init_region`].
//! `dead_code` still covers a few codegen helpers reached only through that
//! example target.
#![allow(dead_code)]

use std::fmt::Write as _;

pub const RCP_LUT: [u32; 128] = [
    0x00010000, 0x0000FE04, 0x0000FC10, 0x0000FA23, 0x0000F83E, 0x0000F660, 0x0000F48A, 0x0000F2BA,
    0x0000F0F1, 0x0000EF2F, 0x0000ED73, 0x0000EBBE, 0x0000EA0F, 0x0000E866, 0x0000E6C3, 0x0000E526,
    0x0000E38E, 0x0000E1FC, 0x0000E070, 0x0000DEE9, 0x0000DD68, 0x0000DBEB, 0x0000DA74, 0x0000D902,
    0x0000D794, 0x0000D62C, 0x0000D4C7, 0x0000D368, 0x0000D20D, 0x0000D0B7, 0x0000CF64, 0x0000CE17,
    0x0000CCCD, 0x0000CB87, 0x0000CA46, 0x0000C908, 0x0000C7CE, 0x0000C698, 0x0000C566, 0x0000C437,
    0x0000C30C, 0x0000C1E5, 0x0000C0C1, 0x0000BFA0, 0x0000BE83, 0x0000BD69, 0x0000BC52, 0x0000BB3F,
    0x0000BA2F, 0x0000B921, 0x0000B817, 0x0000B710, 0x0000B60B, 0x0000B50A, 0x0000B40B, 0x0000B30F,
    0x0000B216, 0x0000B120, 0x0000B02C, 0x0000AF3B, 0x0000AE4C, 0x0000AD60, 0x0000AC77, 0x0000AB8F,
    0x0000AAAB, 0x0000A9C8, 0x0000A8E8, 0x0000A80B, 0x0000A72F, 0x0000A656, 0x0000A57F, 0x0000A4AA,
    0x0000A3D7, 0x0000A306, 0x0000A238, 0x0000A16B, 0x0000A0A1, 0x00009FD8, 0x00009F11, 0x00009E4D,
    0x00009D8A, 0x00009CC9, 0x00009C0A, 0x00009B4C, 0x00009A91, 0x000099D7, 0x0000991F, 0x00009869,
    0x000097B4, 0x00009701, 0x00009650, 0x000095A0, 0x000094F2, 0x00009446, 0x0000939B, 0x000092F1,
    0x00009249, 0x000091A3, 0x000090FE, 0x0000905A, 0x00008FB8, 0x00008F17, 0x00008E78, 0x00008DDA,
    0x00008D3E, 0x00008CA3, 0x00008C09, 0x00008B70, 0x00008AD9, 0x00008A43, 0x000089AE, 0x0000891B,
    0x00008889, 0x000087F8, 0x00008768, 0x000086D9, 0x0000864C, 0x000085BF, 0x00008534, 0x000084AA,
    0x00008421, 0x00008399, 0x00008312, 0x0000828D, 0x00008208, 0x00008185, 0x00008102, 0x00008081,
];

pub const RSQRT_EVEN_LUT: [u32; 128] = [
    0x00010000, 0x0000FF01, 0x0000FE06, 0x0000FD0D, 0x0000FC17, 0x0000FB24, 0x0000FA34, 0x0000F946,
    0x0000F85B, 0x0000F773, 0x0000F68D, 0x0000F5A9, 0x0000F4C8, 0x0000F3EA, 0x0000F30E, 0x0000F234,
    0x0000F15C, 0x0000F087, 0x0000EFB3, 0x0000EEE2, 0x0000EE13, 0x0000ED46, 0x0000EC7C, 0x0000EBB3,
    0x0000EAEC, 0x0000EA27, 0x0000E964, 0x0000E8A3, 0x0000E7E4, 0x0000E727, 0x0000E66B, 0x0000E5B1,
    0x0000E4F9, 0x0000E443, 0x0000E38E, 0x0000E2DB, 0x0000E22A, 0x0000E17A, 0x0000E0CC, 0x0000E020,
    0x0000DF75, 0x0000DECB, 0x0000DE23, 0x0000DD7C, 0x0000DCD7, 0x0000DC34, 0x0000DB92, 0x0000DAF1,
    0x0000DA51, 0x0000D9B3, 0x0000D916, 0x0000D87B, 0x0000D7E1, 0x0000D748, 0x0000D6B0, 0x0000D61A,
    0x0000D585, 0x0000D4F1, 0x0000D45E, 0x0000D3CD, 0x0000D33C, 0x0000D2AD, 0x0000D21F, 0x0000D192,
    0x0000D106, 0x0000D07B, 0x0000CFF1, 0x0000CF69, 0x0000CEE1, 0x0000CE5A, 0x0000CDD5, 0x0000CD50,
    0x0000CCCD, 0x0000CC4A, 0x0000CBC9, 0x0000CB48, 0x0000CAC8, 0x0000CA49, 0x0000C9CC, 0x0000C94F,
    0x0000C8D3, 0x0000C858, 0x0000C7DD, 0x0000C764, 0x0000C6EB, 0x0000C674, 0x0000C5FD, 0x0000C587,
    0x0000C512, 0x0000C49D, 0x0000C42A, 0x0000C3B7, 0x0000C345, 0x0000C2D4, 0x0000C263, 0x0000C1F4,
    0x0000C185, 0x0000C116, 0x0000C0A9, 0x0000C03C, 0x0000BFD0, 0x0000BF65, 0x0000BEFA, 0x0000BE90,
    0x0000BE27, 0x0000BDBE, 0x0000BD56, 0x0000BCEF, 0x0000BC89, 0x0000BC23, 0x0000BBBD, 0x0000BB59,
    0x0000BAF5, 0x0000BA91, 0x0000BA2F, 0x0000B9CC, 0x0000B96B, 0x0000B90A, 0x0000B8A9, 0x0000B84A,
    0x0000B7EA, 0x0000B78C, 0x0000B72E, 0x0000B6D0, 0x0000B673, 0x0000B617, 0x0000B5BB, 0x0000B560,
];

pub const RSQRT_ODD_LUT: [u32; 128] = [
    0x0000B505, 0x0000B451, 0x0000B39F, 0x0000B2EF, 0x0000B241, 0x0000B196, 0x0000B0EC, 0x0000B044,
    0x0000AF9D, 0x0000AEF9, 0x0000AE56, 0x0000ADB6, 0x0000AD16, 0x0000AC79, 0x0000ABDD, 0x0000AB43,
    0x0000AAAB, 0x0000AA14, 0x0000A97E, 0x0000A8EB, 0x0000A858, 0x0000A7C7, 0x0000A738, 0x0000A6AA,
    0x0000A61D, 0x0000A592, 0x0000A508, 0x0000A480, 0x0000A3F9, 0x0000A373, 0x0000A2EE, 0x0000A26B,
    0x0000A1E9, 0x0000A168, 0x0000A0E8, 0x0000A069, 0x00009FEC, 0x00009F70, 0x00009EF5, 0x00009E7B,
    0x00009E02, 0x00009D8A, 0x00009D13, 0x00009C9D, 0x00009C29, 0x00009BB5, 0x00009B42, 0x00009AD0,
    0x00009A60, 0x000099F0, 0x00009981, 0x00009913, 0x000098A6, 0x0000983A, 0x000097CF, 0x00009764,
    0x000096FB, 0x00009692, 0x0000962B, 0x000095C4, 0x0000955E, 0x000094F8, 0x00009494, 0x00009430,
    0x000093CD, 0x0000936B, 0x0000930A, 0x000092A9, 0x00009249, 0x000091EA, 0x0000918C, 0x0000912E,
    0x000090D1, 0x00009074, 0x00009019, 0x00008FBE, 0x00008F64, 0x00008F0A, 0x00008EB1, 0x00008E59,
    0x00008E01, 0x00008DAA, 0x00008D53, 0x00008CFD, 0x00008CA8, 0x00008C54, 0x00008C00, 0x00008BAC,
    0x00008B59, 0x00008B07, 0x00008AB5, 0x00008A64, 0x00008A13, 0x000089C3, 0x00008974, 0x00008925,
    0x000088D6, 0x00008889, 0x0000883B, 0x000087EE, 0x000087A2, 0x00008756, 0x0000870B, 0x000086C0,
    0x00008675, 0x0000862B, 0x000085E2, 0x00008599, 0x00008550, 0x00008508, 0x000084C1, 0x00008479,
    0x00008433, 0x000083EC, 0x000083A7, 0x00008361, 0x0000831C, 0x000082D8, 0x00008293, 0x00008250,
    0x0000820C, 0x000081C9, 0x00008187, 0x00008145, 0x00008103, 0x000080C2, 0x00008081, 0x00008040,
];

pub const SINCOS_LUT: [u32; 257] = [
    0x00000000, 0x00000192, 0x00000324, 0x000004B6, 0x00000648, 0x000007DA, 0x0000096C, 0x00000AFE,
    0x00000C90, 0x00000E21, 0x00000FB3, 0x00001144, 0x000012D5, 0x00001466, 0x000015F7, 0x00001787,
    0x00001918, 0x00001AA8, 0x00001C38, 0x00001DC7, 0x00001F56, 0x000020E5, 0x00002274, 0x00002402,
    0x00002590, 0x0000271E, 0x000028AB, 0x00002A38, 0x00002BC4, 0x00002D50, 0x00002EDC, 0x00003067,
    0x000031F1, 0x0000337C, 0x00003505, 0x0000368E, 0x00003817, 0x0000399F, 0x00003B27, 0x00003CAE,
    0x00003E34, 0x00003FBA, 0x0000413F, 0x000042C3, 0x00004447, 0x000045CB, 0x0000474D, 0x000048CF,
    0x00004A50, 0x00004BD1, 0x00004D50, 0x00004ECF, 0x0000504D, 0x000051CB, 0x00005348, 0x000054C3,
    0x0000563E, 0x000057B9, 0x00005932, 0x00005AAA, 0x00005C22, 0x00005D99, 0x00005F0F, 0x00006084,
    0x000061F8, 0x0000636B, 0x000064DD, 0x0000664E, 0x000067BE, 0x0000692D, 0x00006A9B, 0x00006C08,
    0x00006D74, 0x00006EDF, 0x00007049, 0x000071B2, 0x0000731A, 0x00007480, 0x000075E6, 0x0000774A,
    0x000078AD, 0x00007A10, 0x00007B70, 0x00007CD0, 0x00007E2F, 0x00007F8C, 0x000080E8, 0x00008243,
    0x0000839C, 0x000084F5, 0x0000864C, 0x000087A1, 0x000088F6, 0x00008A49, 0x00008B9A, 0x00008CEB,
    0x00008E3A, 0x00008F88, 0x000090D4, 0x0000921F, 0x00009368, 0x000094B0, 0x000095F7, 0x0000973C,
    0x00009880, 0x000099C2, 0x00009B03, 0x00009C42, 0x00009D80, 0x00009EBC, 0x00009FF7, 0x0000A130,
    0x0000A268, 0x0000A39E, 0x0000A4D2, 0x0000A605, 0x0000A736, 0x0000A866, 0x0000A994, 0x0000AAC1,
    0x0000ABEB, 0x0000AD14, 0x0000AE3C, 0x0000AF62, 0x0000B086, 0x0000B1A8, 0x0000B2C9, 0x0000B3E8,
    0x0000B505, 0x0000B620, 0x0000B73A, 0x0000B852, 0x0000B968, 0x0000BA7D, 0x0000BB8F, 0x0000BCA0,
    0x0000BDAF, 0x0000BEBC, 0x0000BFC7, 0x0000C0D1, 0x0000C1D8, 0x0000C2DE, 0x0000C3E2, 0x0000C4E4,
    0x0000C5E4, 0x0000C6E2, 0x0000C7DE, 0x0000C8D9, 0x0000C9D1, 0x0000CAC7, 0x0000CBBC, 0x0000CCAE,
    0x0000CD9F, 0x0000CE8E, 0x0000CF7A, 0x0000D065, 0x0000D14D, 0x0000D234, 0x0000D318, 0x0000D3FB,
    0x0000D4DB, 0x0000D5BA, 0x0000D696, 0x0000D770, 0x0000D848, 0x0000D91E, 0x0000D9F2, 0x0000DAC4,
    0x0000DB94, 0x0000DC62, 0x0000DD2D, 0x0000DDF7, 0x0000DEBE, 0x0000DF83, 0x0000E046, 0x0000E107,
    0x0000E1C6, 0x0000E282, 0x0000E33C, 0x0000E3F4, 0x0000E4AA, 0x0000E55E, 0x0000E610, 0x0000E6BF,
    0x0000E76C, 0x0000E817, 0x0000E8BF, 0x0000E966, 0x0000EA0A, 0x0000EAAB, 0x0000EB4B, 0x0000EBE8,
    0x0000EC83, 0x0000ED1C, 0x0000EDB3, 0x0000EE47, 0x0000EED9, 0x0000EF68, 0x0000EFF5, 0x0000F080,
    0x0000F109, 0x0000F18F, 0x0000F213, 0x0000F295, 0x0000F314, 0x0000F391, 0x0000F40C, 0x0000F484,
    0x0000F4FA, 0x0000F56E, 0x0000F5DF, 0x0000F64E, 0x0000F6BA, 0x0000F724, 0x0000F78C, 0x0000F7F1,
    0x0000F854, 0x0000F8B4, 0x0000F913, 0x0000F96E, 0x0000F9C8, 0x0000FA1F, 0x0000FA73, 0x0000FAC5,
    0x0000FB15, 0x0000FB62, 0x0000FBAD, 0x0000FBF5, 0x0000FC3B, 0x0000FC7F, 0x0000FCC0, 0x0000FCFE,
    0x0000FD3B, 0x0000FD74, 0x0000FDAC, 0x0000FDE1, 0x0000FE13, 0x0000FE43, 0x0000FE71, 0x0000FE9C,
    0x0000FEC4, 0x0000FEEB, 0x0000FF0E, 0x0000FF30, 0x0000FF4E, 0x0000FF6B, 0x0000FF85, 0x0000FF9C,
    0x0000FFB1, 0x0000FFC4, 0x0000FFD4, 0x0000FFE1, 0x0000FFEC, 0x0000FFF5, 0x0000FFFB, 0x0000FFFF,
    0x00010000,
];

/// Physical address of the RCP table inside mirror A of the register-file
/// BSRAM (design `fpu-design-v2` section 9.4).
pub const MIRROR_A_RCP_BASE: usize = 128;

/// Physical address of the 256 packed SINCOS intervals inside mirror A.
pub const MIRROR_A_SINCOS_BASE: usize = 256;

/// Physical address of the even-exponent RSQRT table inside mirror B.
pub const MIRROR_B_RSQRT_EVEN_BASE: usize = 128;

/// Physical address of the odd-exponent RSQRT table inside mirror B, right
/// after the even table's 128 entries.
pub const MIRROR_B_RSQRT_ODD_BASE: usize = 256;

/// Marker that opens the generated BSRAM initialization region in
/// `cpu_v3_fpu_register_ram.v`.
pub const LUT_INIT_BEGIN: &str = "// LUT_INIT_BEGIN (generated by fpu_lut_codegen; do not edit)";

/// Marker that closes the generated BSRAM initialization region.
pub const LUT_INIT_END: &str = "// LUT_INIT_END";

/// Number of assignments per line in the generated region.
const LUT_WORDS_PER_LINE: usize = 4;

/// Appends one mirror's nonzero table entries to `output`, four per line.
fn push_table(output: &mut String, mirror: &str, base: usize, table: &[u32]) {
    let entries: Vec<(usize, u32)> = table
        .iter()
        .enumerate()
        .filter(|(_, value)| **value != 0)
        .map(|(index, value)| (base + index, *value))
        .collect();
    for chunk in entries.chunks(LUT_WORDS_PER_LINE) {
        output.push_str("    ");
        for (column, (address, value)) in chunk.iter().enumerate() {
            if column != 0 {
                output.push(' ');
            }
            write!(output, "{mirror}[{address}] = 32'h{value:08X};").unwrap();
        }
        output.push('\n');
    }
}

/// Packs one interpolation interval into a register-file word.
///
/// Every special-function sample is an unsigned 17-bit Q16.16 value and the
/// measured delta bounds fit signed 10 bits (`-508..=402` across all tables).
fn pack_interval(current: u32, next: u32) -> u32 {
    assert!(current <= 0x1_0000);
    assert!(next <= 0x1_0000);
    let delta = next as i32 - current as i32;
    assert!((-512..=511).contains(&delta));
    current | (((delta as u32) & 0x03FF) << 17)
}

fn pack_with_endpoint(table: &[u32], endpoint: u32) -> Vec<u32> {
    table
        .iter()
        .enumerate()
        .map(|(index, &current)| {
            let next = table.get(index + 1).copied().unwrap_or(endpoint);
            pack_interval(current, next)
        })
        .collect()
}

pub(crate) fn packed_rcp_lut() -> Vec<u32> {
    pack_with_endpoint(&RCP_LUT, RCP_ENDPOINT as u32)
}

pub(crate) fn packed_rsqrt_even_lut() -> Vec<u32> {
    pack_with_endpoint(&RSQRT_EVEN_LUT, RSQRT_EVEN_ENDPOINT as u32)
}

pub(crate) fn packed_rsqrt_odd_lut() -> Vec<u32> {
    pack_with_endpoint(&RSQRT_ODD_LUT, RSQRT_ODD_ENDPOINT as u32)
}

pub(crate) fn packed_sincos_lut() -> Vec<u32> {
    SINCOS_LUT
        .windows(2)
        .map(|samples| pack_interval(samples[0], samples[1]))
        .collect()
}

/// Renders the complete generated BSRAM initialization region (markers
/// included) for `cpu_v3_fpu_register_ram.v`. The zero-fill of the whole
/// 512-word array and every nonzero LUT entry live in one `initial` block:
/// separate blocks would race, and the hidden region is read-only at runtime.
///
/// This is the single renderer behind both the `fpu_lut_codegen` example
/// (which rewrites the `.v` in place) and the drift test below.
pub fn render_lut_init_region() -> String {
    let mut output = String::new();
    output.push_str(LUT_INIT_BEGIN);
    output.push('\n');
    output.push_str("initial begin\n");
    output.push_str(
        "    for (initial_word = 0; initial_word < 512; initial_word = initial_word + 1) begin\n",
    );
    output.push_str("        mirror_0[initial_word] = 0;\n");
    output.push_str("        mirror_1[initial_word] = 0;\n");
    output.push_str("    end\n");
    push_table(
        &mut output,
        "mirror_0",
        MIRROR_A_RCP_BASE,
        &packed_rcp_lut(),
    );
    push_table(
        &mut output,
        "mirror_0",
        MIRROR_A_SINCOS_BASE,
        &packed_sincos_lut(),
    );
    push_table(
        &mut output,
        "mirror_1",
        MIRROR_B_RSQRT_EVEN_BASE,
        &packed_rsqrt_even_lut(),
    );
    push_table(
        &mut output,
        "mirror_1",
        MIRROR_B_RSQRT_ODD_BASE,
        &packed_rsqrt_odd_lut(),
    );
    output.push_str("end\n");
    output.push_str(LUT_INIT_END);
    output
}

/// Returns the generated region of `source` verbatim, from the begin marker
/// through the end marker. Panics if either marker is missing.
pub fn extract_lut_init_region(source: &str) -> &str {
    let begin = source
        .find(LUT_INIT_BEGIN)
        .expect("LUT_INIT_BEGIN marker is missing");
    let end = source
        .find(LUT_INIT_END)
        .expect("LUT_INIT_END marker is missing")
        + LUT_INIT_END.len();
    &source[begin..end]
}

/// Rewrites the generated region of `source` in place, leaving everything
/// outside the markers untouched. Idempotent.
pub fn replace_lut_init_region(source: &str) -> String {
    let begin = source
        .find(LUT_INIT_BEGIN)
        .expect("LUT_INIT_BEGIN marker is missing");
    let end = source
        .find(LUT_INIT_END)
        .expect("LUT_INIT_END marker is missing")
        + LUT_INIT_END.len();
    let mut output = String::with_capacity(source.len());
    output.push_str(&source[..begin]);
    output.push_str(&render_lut_init_region());
    output.push_str(&source[end..]);
    output
}

/// RCP endpoint: `1/m` at `m = 2`, i.e. index 127's `next`. The 128-entry table
/// does not store it, so the datapath supplies the exact constant instead of
/// reading out of bounds.
const RCP_ENDPOINT: i32 = 0x8000;

/// RSQRT even-table endpoint: `1/sqrt(m)` at `m = 2`, i.e. index 127's `next`.
/// The 128-entry table covers `m = 1 + i/128` up to `1.9921875`, so
/// `round(2^16/sqrt(2))` is supplied as the final interpolation endpoint.
const RSQRT_EVEN_ENDPOINT: i32 = 0xB505;

/// RSQRT odd-table endpoint: `1/sqrt(2m)` at `m = 2` is exactly `0.5`, i.e.
/// index 127's `next`.
const RSQRT_ODD_ENDPOINT: i32 = 0x8000;

/// `round(2/pi * 2^16)`, the leading Q16.16 term of the two-term SINCOS range
/// reduction (revised freeze, design section 9.4).
const TWO_OVER_PI_C0: i32 = 41_722;

/// `round((2/pi - C0 * 2^-16) * 2^32)`, the residual term. It is negative
/// because `C0` rounded up; `|C1| < 2^16` keeps every partial product 16x16
/// class. The represented constant `C0*2^-16 + C1*2^-32` differs from `2/pi` by
/// ~7.1e-11, i.e. `< 0.16` LSB of the reduced argument `t` over the full i32
/// input range.
const TWO_OVER_PI_C1: i32 = -31_890;

/// Linear interpolation shared by RCP and both RSQRT tables (128 entries,
/// 9-bit residue, `>> 9`). The stored table stops one entry short of the
/// interval's right edge, so the final entry's `next` comes from `endpoint`
/// instead of reading out of bounds.
#[inline]
fn interpolate(lut: &[u32], index: usize, residue: i32, shift: u32, endpoint: i32) -> i32 {
    let current = lut[index] as i32;
    let next = if index + 1 < lut.len() {
        lut[index + 1] as i32
    } else {
        endpoint
    };
    let delta = next - current;
    current + ((delta * residue) >> shift)
}

/// RCP approximation of `x` in Q16.16 (design section 9.4):
///
/// * `sign out = sign in`, `x == 0 -> 0x7FFF_FFFF`;
/// * normalize `|x| = m * 2^e`, `m in [1,2)` (CLZ on the magnitude);
/// * index the LUT with the top 7 fraction bits of `m`, interpolate with the
///   next 9 bits, then rescale by `2^-e` with a saturating barrel shift.
pub fn rcp_q16(x: i32) -> i32 {
    if x == 0 {
        return i32::MAX;
    }
    let negative = x < 0;
    let magnitude = x.unsigned_abs();
    let clz = magnitude.leading_zeros();
    let normalized = magnitude << clz;
    let index = ((normalized >> 24) & 0x7F) as usize;
    let residue = ((normalized >> 15) & 0x1FF) as i32;
    let interpolated = interpolate(&RCP_LUT, index, residue, 9, RCP_ENDPOINT);
    // Real exponent of |x| is e = clz - 16, so 1/|x| rescales by 2^(15 - clz).
    let shift = clz as i32 - 15;
    let scaled = if shift >= 0 {
        let wide = (interpolated as u64) << shift;
        if wide > i32::MAX as u64 {
            i32::MAX
        } else {
            wide as i32
        }
    } else {
        interpolated >> (-shift)
    };
    if negative {
        -scaled
    } else {
        scaled
    }
}

/// RSQRT approximation of `x` in Q16.16 (revised freeze, design section 9.4):
/// `x <= 0 -> 0`, otherwise normalize `|x| = m * 2^e` with `m in [1,2)`
/// (CLZ, same shape as [`rcp_q16`]) and pick the table by the parity of `e`:
/// even uses [`RSQRT_EVEN_LUT`] (`1/sqrt(m)`) and odd uses [`RSQRT_ODD_LUT`]
/// (`1/sqrt(2m)`). The top 7 fraction bits of `m` index a 128-entry table, the
/// next 9 bits are the interpolation residue, and the result rescales by
/// `2^(-floor(e/2))`. There is **no** division in the datapath and no Newton
/// step: the index and residue are bit slices of the normalized magnitude and
/// the tables are fine enough to meet the 2 ulp target.
///
/// Index 127's `next` comes from the exact endpoint instead of reading out of
/// bounds: `round(2^16/sqrt(2))` for the even table and `0.5` for the odd.
pub fn rsqrt_q16(x: i32) -> i32 {
    if x <= 0 {
        return 0;
    }
    let magnitude = x as u32;
    let clz = magnitude.leading_zeros();
    let binary_exponent = 31 - clz;
    let odd = binary_exponent & 1 != 0;
    // `normalized = m * 2^31` with `m in [1,2)`: the top 7 fraction bits index
    // the table and the next 9 are the residue, exactly like RCP.
    let normalized = magnitude << clz;
    let index = ((normalized >> 24) & 0x7F) as usize;
    let residue = ((normalized >> 15) & 0x1FF) as i32;
    let (lut, endpoint) = if odd {
        (&RSQRT_ODD_LUT, RSQRT_ODD_ENDPOINT)
    } else {
        (&RSQRT_EVEN_LUT, RSQRT_EVEN_ENDPOINT)
    };
    let interpolated = interpolate(lut, index, residue, 9, endpoint);
    // Real exponent `e = binary_exponent - 16`; `floor(e/2)` is exact because
    // `binary_exponent - 16 - odd` is always even.
    let shift = (binary_exponent as i32 - 16 - i32::from(odd)) / 2;
    if shift >= 0 {
        interpolated >> shift
    } else {
        interpolated << (-shift)
    }
}

/// Single sample of `sin(pi/2 * u)` for `u` in Q16.16 over `[0,1]`, from the
/// 257-entry quarter-wave sine table. `u == 1.0` is the exact endpoint entry,
/// which the `0x1_0000 - f` quadrant reflection needs when `f == 0`.
#[inline]
fn sine_quarter(u: i32) -> i32 {
    if u >= 0x1_0000 {
        return SINCOS_LUT[256] as i32;
    }
    let index = ((u >> 8) & 0xFF) as usize;
    let residue = u & 0xFF;
    let current = SINCOS_LUT[index] as i32;
    let next = SINCOS_LUT[index + 1] as i32;
    let delta = next - current;
    current + ((delta * residue) >> 8)
}

/// `sin(pi/2 * (quadrant + fraction))` via the quarter-wave symmetries.
#[inline]
fn quadrant_sine(quadrant: i32, fraction: i32) -> i32 {
    match quadrant & 3 {
        0 => sine_quarter(fraction),
        1 => sine_quarter(0x1_0000 - fraction),
        2 => -sine_quarter(fraction),
        _ => -sine_quarter(0x1_0000 - fraction),
    }
}

/// SINCOS of `a` (radians, Q16.16), returning `(sin, cos)`.
///
/// There is no input clamp: any `i32` is evaluated at its actual angle. Range
/// reduction evaluates `t = floor(a * (2/pi))` in Q16.16 with the two-term
/// constant `2/pi ~= C0*2^-16 + C1*2^-32` ([`TWO_OVER_PI_C0`],
/// [`TWO_OVER_PI_C1`]). Splitting `a = a_hi*2^16 + a_lo` (`a_hi = a >> 16`
/// arithmetic in `[-32768, 32767]`, `a_lo` the unsigned low 16 bits in
/// `[0, 65535]`) keeps the whole computation inside `i64` for the full range:
///
/// ```text
/// p0 = a_lo * C0            (unsigned, |p0| <= 2.73e9)
/// p1 = a_lo * C1            (signed,   |p1| <= 2.09e9)
/// h0 = a_hi * C0            (signed,   |h0| <= 1.37e9)
/// h1 = a_hi * C1            (signed,   |h1| <= 1.05e9)
/// t  = h0 + (((p0 + h1) << 16) + p1) >> 32
/// ```
///
/// `|p0 + h1| <= 3.78e9`, so the shift peaks near `2^47.8`; the reduced value
/// itself satisfies `|t| <= ceil(2^31 * 2/pi) + 1 < 2^30.4 < i32::MAX`, which
/// the `debug_assert!` below confirms on every debug/test build (notably for
/// `a = i32::MIN`/`i32::MAX`).
///
/// The numerator `(a*C0 << 16) + a*C1` expands to
/// `(h0 << 32) + ((p0 + h1) << 16) + p1`, and the final `>> 32` is an
/// arithmetic shift, so the formula is `floor(a * represented_constant)`: the
/// only truncation is the final shift (< 1 LSB) plus the constant error
/// (< 0.16 LSB over the full range). Negative `a` works because `a_hi`, `h0`,
/// `h1` and `p1` are signed, `p0` is unsigned, and the arithmetic shift floors.
///
/// The 4 LSB accuracy guarantee holds for `|a| <= 45` rad; outside it the
/// result is still the actual `sin`/`cos` (the argument never wraps or
/// saturates), but the error grows slowly with `|a|` and is only bounded by the
/// measured full-range test.
///
/// `q = (t >> 16) & 3`, `f = t & 0xFFFF`; `sin` uses the quarter-wave table at
/// `(q, f)` and `cos` at `(q + 1, f)` ([`quadrant_sine`]).
pub fn sincos_q16(a: i32) -> (i32, i32) {
    let a_hi = i64::from(a >> 16);
    let a_lo = i64::from(a & 0xFFFF);
    let c0 = i64::from(TWO_OVER_PI_C0);
    let c1 = i64::from(TWO_OVER_PI_C1);
    let h0 = a_hi * c0;
    let h1 = a_hi * c1;
    let p0 = a_lo * c0;
    let p1 = a_lo * c1;
    let t_wide = h0 + ((((p0 + h1) << 16) + p1) >> 32);
    debug_assert!(
        t_wide >= i64::from(i32::MIN) && t_wide <= i64::from(i32::MAX),
        "SINCOS range reduction t={t_wide} escaped i32 for a={a}"
    );
    let t = t_wide as i32;
    let quadrant = (t >> 16) & 3;
    let fraction = t & 0xFFFF;
    (
        quadrant_sine(quadrant, fraction),
        quadrant_sine(quadrant + 1, fraction),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ulp_of(shift: i32) -> f64 {
        if shift > 0 {
            (1u64 << shift) as f64
        } else {
            1.0
        }
    }

    fn rcp_shift(x: i32) -> i32 {
        x.unsigned_abs().leading_zeros() as i32 - 15
    }

    fn rsqrt_scale_shift(x: i32) -> i32 {
        let magnitude = x as u32;
        let binary_exponent = 31 - magnitude.leading_zeros();
        let exponent = binary_exponent as i32 - 16;
        if exponent & 1 == 0 {
            exponent / 2
        } else {
            (exponent - 1) / 2
        }
    }

    /// `(max(|sin error|, |cos error|) in Q16.16 LSB, sin, cos)` for the
    /// integer model against `f64`. The input `a` is an exact multiple of
    /// `2^-16`, so the reference is trustworthy at every magnitude.
    fn sincos_abs_error(a: i32) -> (i32, i32, i32) {
        let radians = f64::from(a) / 65536.0;
        let (sin, cos) = sincos_q16(a);
        let sin_exact = (radians.sin() * 65536.0).round() as i32;
        let cos_exact = (radians.cos() * 65536.0).round() as i32;
        (
            (sin - sin_exact).abs().max((cos - cos_exact).abs()),
            sin,
            cos,
        )
    }

    /// Deterministic xorshift64* used to sample the full `i32` input range.
    fn xorshift64(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    #[test]
    fn lut_literals_match_f64_generation() {
        for (i, &entry) in RCP_LUT.iter().enumerate() {
            let rcp = ((1.0 / (1.0 + i as f64 / 128.0)) * 65536.0).round() as u32;
            assert_eq!(entry, rcp, "RCP_LUT[{i}]");
        }
        for (i, &entry) in RSQRT_EVEN_LUT.iter().enumerate() {
            let rsqrt = ((1.0 / (1.0 + i as f64 / 128.0).sqrt()) * 65536.0).round() as u32;
            assert_eq!(entry, rsqrt, "RSQRT_EVEN_LUT[{i}]");
        }
        for (i, &entry) in RSQRT_ODD_LUT.iter().enumerate() {
            let rsqrt = ((1.0 / (2.0 * (1.0 + i as f64 / 128.0)).sqrt()) * 65536.0).round() as u32;
            assert_eq!(entry, rsqrt, "RSQRT_ODD_LUT[{i}]");
        }
        for (i, &entry) in SINCOS_LUT.iter().enumerate() {
            let sine =
                ((std::f64::consts::PI / 2.0 * i as f64 / 256.0).sin() * 65536.0).round() as u32;
            assert_eq!(entry, sine, "SINCOS_LUT[{i}]");
        }
        assert_eq!(RCP_LUT[0], 0x1_0000);
        assert_eq!(RSQRT_EVEN_LUT[0], 0x1_0000);
        assert_eq!(RSQRT_ODD_LUT[0], 0xB505);
        assert_eq!(SINCOS_LUT[0], 0);
        assert_eq!(SINCOS_LUT[256], 0x1_0000);
        assert_eq!(RSQRT_EVEN_ENDPOINT, 0xB505);
        assert_eq!(RSQRT_ODD_ENDPOINT, 0x8000);
    }

    /// The checked-in register-file Verilog must carry exactly the region the
    /// renderer produces, so the file cannot drift from these tables silently.
    #[test]
    fn register_ram_lut_init_matches_rendered_region() {
        let source = include_str!("../hardware/fpu/cpu_v3_fpu_register_ram.v");
        assert_eq!(
            extract_lut_init_region(source),
            render_lut_init_region(),
            "cpu_v3_fpu_register_ram.v LUT init region is stale; \
             run `cargo run -p cpu-v3 --example fpu_lut_codegen`"
        );
    }

    #[test]
    fn lut_init_region_rewrite_is_idempotent() {
        let source = format!("module m;\n{LUT_INIT_BEGIN}\n{LUT_INIT_END}\nendmodule\n");
        let once = replace_lut_init_region(&source);
        assert_eq!(extract_lut_init_region(&once), render_lut_init_region());
        assert_eq!(replace_lut_init_region(&once), once);
    }

    #[test]
    fn range_reduction_constants_match_f64() {
        let two_over_pi = 2.0 / std::f64::consts::PI;
        let c0 = (two_over_pi * 65536.0).round() as i32;
        let c1 = ((two_over_pi - f64::from(c0) / 65536.0) * 4294967296.0).round() as i32;
        assert_eq!(TWO_OVER_PI_C0, c0);
        assert_eq!(TWO_OVER_PI_C1, c1);
    }

    #[test]
    fn interpolation_deltas_fit_i16() {
        let mut rcp_max = 0i32;
        for i in 0..127 {
            rcp_max = rcp_max.max((RCP_LUT[i + 1] as i32 - RCP_LUT[i] as i32).abs());
        }
        rcp_max = rcp_max.max((RCP_ENDPOINT - RCP_LUT[127] as i32).abs());
        let mut rsqrt_max = 0i32;
        for i in 0..127 {
            rsqrt_max =
                rsqrt_max.max((RSQRT_EVEN_LUT[i + 1] as i32 - RSQRT_EVEN_LUT[i] as i32).abs());
            rsqrt_max =
                rsqrt_max.max((RSQRT_ODD_LUT[i + 1] as i32 - RSQRT_ODD_LUT[i] as i32).abs());
        }
        rsqrt_max = rsqrt_max.max((RSQRT_EVEN_ENDPOINT - RSQRT_EVEN_LUT[127] as i32).abs());
        rsqrt_max = rsqrt_max.max((RSQRT_ODD_ENDPOINT - RSQRT_ODD_LUT[127] as i32).abs());
        let mut sincos_max = 0i32;
        for i in 0..256 {
            sincos_max = sincos_max.max(SINCOS_LUT[i + 1] as i32 - SINCOS_LUT[i] as i32);
        }
        eprintln!(
            "max interpolation |delta|: rcp={rcp_max}, rsqrt={rsqrt_max}, sincos={sincos_max}"
        );
        assert!(rcp_max <= i32::from(i16::MAX));
        assert!(rsqrt_max <= i32::from(i16::MAX));
        assert!(sincos_max <= i32::from(i16::MAX));
    }

    #[test]
    fn packed_intervals_round_trip_samples_and_signed_10_bit_deltas() {
        fn check(words: &[u32], samples: &[u32], endpoint: Option<u32>) {
            assert_eq!(words.len(), samples.len() - usize::from(endpoint.is_none()));
            for (index, &word) in words.iter().enumerate() {
                let current = word & 0x1_FFFF;
                let delta_bits = ((word >> 17) & 0x03FF) as i32;
                let delta = (delta_bits << 22) >> 22;
                let next = samples
                    .get(index + 1)
                    .copied()
                    .or(endpoint)
                    .expect("packed interval must have a next sample");
                assert_eq!(current, samples[index]);
                assert_eq!(delta, next as i32 - current as i32);
                assert!((-512..=511).contains(&delta));
                assert_eq!(word >> 27, 0);
            }
        }

        check(&packed_rcp_lut(), &RCP_LUT, Some(RCP_ENDPOINT as u32));
        check(
            &packed_rsqrt_even_lut(),
            &RSQRT_EVEN_LUT,
            Some(RSQRT_EVEN_ENDPOINT as u32),
        );
        check(
            &packed_rsqrt_odd_lut(),
            &RSQRT_ODD_LUT,
            Some(RSQRT_ODD_ENDPOINT as u32),
        );
        check(&packed_sincos_lut(), &SINCOS_LUT, None);
    }

    #[test]
    fn packed_interpolation_is_bit_exact_for_every_index_and_residue() {
        fn packed_interpolate(word: u32, residue: i32, shift: u32) -> i32 {
            let current = (word & 0x1_FFFF) as i32;
            let delta_bits = ((word >> 17) & 0x03FF) as i32;
            let delta = (delta_bits << 22) >> 22;
            current + ((delta * residue) >> shift)
        }

        for (packed, samples, endpoint) in [
            (packed_rcp_lut(), &RCP_LUT[..], RCP_ENDPOINT),
            (
                packed_rsqrt_even_lut(),
                &RSQRT_EVEN_LUT[..],
                RSQRT_EVEN_ENDPOINT,
            ),
            (
                packed_rsqrt_odd_lut(),
                &RSQRT_ODD_LUT[..],
                RSQRT_ODD_ENDPOINT,
            ),
        ] {
            for (index, &word) in packed.iter().enumerate() {
                for residue in 0..512 {
                    assert_eq!(
                        packed_interpolate(word, residue, 9),
                        interpolate(samples, index, residue, 9, endpoint),
                        "index={index} residue={residue}"
                    );
                }
            }
        }

        let packed = packed_sincos_lut();
        for (index, &word) in packed.iter().enumerate() {
            for residue in 0..256 {
                assert_eq!(
                    packed_interpolate(word, residue, 8),
                    interpolate(&SINCOS_LUT, index, residue, 8, SINCOS_LUT[256] as i32),
                    "sincos index={index} residue={residue}"
                );
            }
        }
    }

    fn rcp_inputs() -> Vec<i32> {
        let mut inputs = Vec::new();
        for raw in 1..=0x8000i32 {
            inputs.push(raw);
        }
        for bit in 0..16 {
            let power = 1i32 << bit;
            inputs.push(power);
            inputs.push(power + 1);
            if power > 1 {
                inputs.push(power - 1);
            }
        }
        inputs.push(1);
        inputs.push(2);
        inputs.push(3);
        inputs.push(0x7FFF_FFFF);
        inputs.push(0x4000_0000);
        inputs.push(0x2000_0000);
        inputs
    }

    #[test]
    fn rcp_accuracy() {
        let mut max_relative = 0.0f64;
        let mut worst_relative = (0i32, 0.0f64, 0i32);
        let mut max_absolute = 0i32;
        let mut worst_absolute = (0i32, 0i32, 0i32);
        for &magnitude in &rcp_inputs() {
            for &x in &[magnitude, -magnitude] {
                let exact = (1.0 / (f64::from(x) / 65536.0)) * 65536.0;
                if exact.abs() > f64::from(i32::MAX) {
                    continue;
                }
                let got = rcp_q16(x);
                let absolute = (f64::from(got) - exact).abs();
                if absolute > f64::from(max_absolute) {
                    max_absolute = absolute.round() as i32;
                    worst_absolute = (x, exact.round() as i32, got);
                }
                let relative = absolute / ulp_of(rcp_shift(x));
                if relative > max_relative {
                    max_relative = relative;
                    worst_relative = (x, exact, got);
                }
            }
        }
        eprintln!(
            "RCP: max {max_relative:.4} result-ulp at x={} (exact={:.3}, got={}), \
             max {max_absolute} raw Q16.16 LSB at x={} (exact={}, got={})",
            worst_relative.0,
            worst_relative.1,
            worst_relative.2,
            worst_absolute.0,
            worst_absolute.1,
            worst_absolute.2
        );
        assert!(
            max_relative <= 2.0,
            "RCP result-relative error {max_relative} ulp > 2 at x={}",
            worst_relative.0
        );
    }

    #[test]
    fn rsqrt_accuracy() {
        let mut max_relative = 0.0f64;
        let mut worst_relative = (0i32, 0.0f64, 0i32);
        let mut max_absolute = 0i32;
        let mut worst_absolute = (0i32, 0i32, 0i32);
        let mut check = |magnitude: i32| {
            let exact = (1.0 / (f64::from(magnitude) / 65536.0).sqrt()) * 65536.0;
            let got = rsqrt_q16(magnitude);
            let absolute = (f64::from(got) - exact).abs();
            if absolute > f64::from(max_absolute) {
                max_absolute = absolute.round() as i32;
                worst_absolute = (magnitude, exact.round() as i32, got);
            }
            let relative = absolute / ulp_of(-rsqrt_scale_shift(magnitude).min(0));
            if relative > max_relative {
                max_relative = relative;
                worst_relative = (magnitude, exact, got);
            }
        };
        // Dense sweep of the small-magnitude range where the LUT dominates.
        for magnitude in 1..=0x8000i32 {
            check(magnitude);
        }
        // Every power of two and its neighbours catch the exponent boundaries.
        for bit in 0..31 {
            let power = 1i32 << bit;
            check(power);
            check(power + 1);
            if power > 1 {
                check(power - 1);
            }
        }
        // Deterministic stride through the whole positive i32 range.
        let mut magnitude = 0x1_0000i32;
        while let Some(next) = magnitude.checked_add(65_537) {
            check(magnitude);
            magnitude = next;
        }
        check(i32::MAX);
        eprintln!(
            "RSQRT: max {max_relative:.4} result-ulp at x={} (exact={:.3}, got={}), \
             max {max_absolute} raw Q16.16 LSB at x={} (exact={}, got={})",
            worst_relative.0,
            worst_relative.1,
            worst_relative.2,
            worst_absolute.0,
            worst_absolute.1,
            worst_absolute.2
        );
        assert!(
            max_relative <= 2.0,
            "RSQRT result-relative error {max_relative} ulp > 2 at x={}",
            worst_relative.0
        );
    }

    #[test]
    fn sincos_accuracy() {
        // Accuracy target domain: |a| <= 45 rad (Q16.16, dense).
        let limit = 45 << 16;
        let mut max_error = 0i32;
        let mut worst = (0i32, 0i32, 0i32);
        let mut check = |a: i32| {
            let radians = f64::from(a) / 65536.0;
            let (sin, cos) = sincos_q16(a);
            let sin_exact = (radians.sin() * 65536.0).round() as i32;
            let cos_exact = (radians.cos() * 65536.0).round() as i32;
            let error = (sin - sin_exact).abs().max((cos - cos_exact).abs());
            if error > max_error {
                max_error = error;
                worst = (a, sin, cos);
            }
        };
        for a in -limit..=limit {
            check(a);
        }
        // Fine sweep around every quadrant boundary k*pi/2, where sin/cos cross
        // zero and the interpolation kink meets the range-reduction error.
        let quarter = std::f64::consts::FRAC_PI_2 * 65536.0;
        let boundary_count = (f64::from(limit) / quarter).floor() as i32;
        for k in -boundary_count..=boundary_count {
            let center = (f64::from(k) * quarter).round() as i32;
            for offset in -8..=8 {
                let sample = center + offset;
                if sample >= -limit && sample <= limit {
                    check(sample);
                }
            }
        }
        eprintln!(
            "SINCOS: max {max_error} Q16.16 LSB at a={} (sin={}, cos={})",
            worst.0, worst.1, worst.2
        );
        assert!(
            max_error <= 4,
            "SINCOS absolute error {max_error} ulp > 4 at a={}",
            worst.0
        );
    }

    /// Inner-range (|a| <= 2 pi) SINCOS error bound (Q16.16 LSB), the range
    /// that matters for rotation use. Measured value pinned by the test print.
    const SINCOS_INNER_RANGE_ULP: i32 = 4;

    #[test]
    fn sincos_inner_range_accuracy() {
        let limit = (2.0 * std::f64::consts::PI * 65536.0).round() as i32;
        let mut max_error = 0i32;
        let mut worst = (0i32, 0i32, 0i32);
        for a in -limit..=limit {
            let (error, sin, cos) = sincos_abs_error(a);
            if error > max_error {
                max_error = error;
                worst = (a, sin, cos);
            }
        }
        eprintln!(
            "SINCOS inner range (|a|<=2pi): max {max_error} Q16.16 LSB at a={} (sin={}, cos={})",
            worst.0, worst.1, worst.2
        );
        assert!(max_error <= SINCOS_INNER_RANGE_ULP);
    }

    /// Full-range SINCOS error bound (Q16.16 LSB). The clamp is gone, so every
    /// `i32` is reduced at its actual angle. The measured maximum over the dense
    /// domain plus edges, a whole-range stride and 2^20 pseudo-random full-range
    /// points is **3 LSB** (worst at `a = -2948768`, i.e. `-44.9946` rad); the
    /// bound is set one LSB above that, matching the dense `<= 4` margin.
    const SINCOS_FULL_RANGE_ULP: i32 = 4;

    #[test]
    fn sincos_full_range_accuracy() {
        // The 4 LSB guarantee still applies only to |a| <= 45 rad.
        let limit = 45 << 16;
        let mut dense_max = 0i32;
        let mut dense_worst = (0i32, 0i32, 0i32);
        let mut range_max = 0i32;
        let mut range_worst = (0i32, 0i32, 0i32);
        let mut check = |a: i32| {
            let (error, sin, cos) = sincos_abs_error(a);
            if error > range_max {
                range_max = error;
                range_worst = (a, sin, cos);
            }
            if a >= -limit && a <= limit && error > dense_max {
                dense_max = error;
                dense_worst = (a, sin, cos);
            }
        };

        // Dense sweep of the guaranteed domain, exactly as before the clamp was
        // removed (the clamp never applied here).
        for a in -limit..=limit {
            check(a);
        }

        // Every power of two and its neighbours on both signs: the sign and
        // exponent boundaries stress the `a_hi`/`a_lo` split.
        for bit in 0..31 {
            let power = 1i32 << bit;
            check(power);
            check(power + 1);
            check(power - 1);
            check(-power);
            check(-power + 1);
            check(-power - 1);
        }
        for a in [
            i32::MIN,
            i32::MIN + 1,
            i32::MIN + 2,
            i32::MAX,
            i32::MAX - 1,
            i32::MAX - 2,
        ] {
            check(a);
        }

        // Deterministic stride through the whole i32 range.
        let mut a = i32::MIN;
        while let Some(next) = a.checked_add(65_537) {
            check(a);
            a = next;
        }
        check(i32::MAX);

        // 2^20 deterministic pseudo-random full-range points.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..(1 << 20) {
            check(xorshift64(&mut state) as u32 as i32);
        }

        eprintln!(
            "SINCOS full range: dense |a|<=45 max {dense_max} LSB at a={} (sin={}, cos={}); \
             full i32 max {range_max} LSB at a={} (sin={}, cos={})",
            dense_worst.0,
            dense_worst.1,
            dense_worst.2,
            range_worst.0,
            range_worst.1,
            range_worst.2
        );
        assert!(
            dense_max <= 4,
            "SINCOS dense error {dense_max} ulp > 4 at a={}",
            dense_worst.0
        );
        assert!(
            range_max <= SINCOS_FULL_RANGE_ULP,
            "SINCOS full-range error {range_max} ulp > {SINCOS_FULL_RANGE_ULP} at a={}",
            range_worst.0
        );
    }

    #[test]
    fn special_function_domains_are_defined() {
        assert_eq!(rcp_q16(0), i32::MAX);
        assert_eq!(rcp_q16(1), i32::MAX);
        assert_eq!(rcp_q16(-1), -i32::MAX);
        assert_eq!(rcp_q16(0x1_0000), 0x1_0000);
        assert_eq!(rcp_q16(-0x1_0000), -0x1_0000);
        assert_eq!(rsqrt_q16(0), 0);
        assert_eq!(rsqrt_q16(-5), 0);
        assert_eq!(rsqrt_q16(0x1_0000), 0x1_0000);
        assert_eq!(sincos_q16(0), (0, 0x1_0000));
    }
}
