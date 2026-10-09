//! One candidate ledger. Planned lane/slot adapters are billed explicitly and
//! are not promoted to verified runtime allocations by this table.
use super::*;
struct Added {
    name: &'static str,
    ff: u64,
    ram: u64,
    bsram: usize,
    owner: &'static str,
    status: &'static str,
}
pub fn probe(root: &Path, b: &bound::Binding, d: &raw::Calendar) {
    let old = bound::inventory::describe(
        b,
        bound::control::Hardware {
            storage: bound::control::Storage::Packed,
            ..Default::default()
        },
        &timed::Hardware {
            preparation: timed::PreparationMode::BoundStages,
            prefetch: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!((old.ff_bits, old.sdp4_cells, old.bsram), (16001, 92, 4));
    let mut out = fs::File::create(root.join("allocation.csv")).unwrap();
    writeln!(out,"row,old_FF,new_FF,old_RAM16SDP4,new_RAM16SDP4,old_BSRAM,new_BSRAM,status,owner_and_boundary").unwrap();
    let (mut ff, mut ram, mut bsram) = (0, 0, 0);
    for row in &old.rows {
        let (new_ff, new_ram, new_bsram, status) = match row.name {
            "derivative FF" => (
                d.storage_plan.packed.ff_bits as u64,
                0,
                0,
                "streamed calendar and bit replay",
            ),
            "periodic storage phase/valid" => (
                row.ff_bits - b.derivative.packed.control_ff_bits
                    + d.storage_plan.packed.control_ff_bits,
                0,
                0,
                "streamed D replaces only its phase/valid",
            ),
            "shared contexts"
            | "coordinate ready records"
            | "coefficient pass-through"
            | "coefficient ready records"
            | "packet holding records"
            | "slot table"
            | "Group4 FIFO"
            | "color tokens"
            | "color tree/feedback/normalize"
            | "result FIFO" => (
                0,
                0,
                0,
                "replacement below; not native controller integration",
            ),
            _ => (
                row.ff_bits,
                row.sdp4_cells,
                row.bsram,
                "unchanged conservative baseline allocation",
            ),
        };
        writeln!(
            out,
            "{},{},{},{},{},{},{},{},{}",
            csv(row.name),
            row.ff_bits,
            new_ff,
            row.sdp4_cells,
            new_ram,
            row.bsram,
            new_bsram,
            csv(status),
            csv(row.ports)
        )
        .unwrap();
        ff += new_ff;
        ram += new_ram;
        bsram += new_bsram;
    }
    let added=[
        Added{name:"raw U/V banks",ff:0,ram:0,bsram:2,owner:"2x64 used rows of512x36; ingress W / sole D R; BP data belongs to macro",status:"finite port/owner/value/CE replay"},
        Added{name:"raw arithmetic window",ff:320,ram:0,bsram:0,owner:"one4xU/V40 window; no8 full quad copies",status:"actual D read offsets and owner replay"},
        Added{name:"raw ingress high nibble",ff:8,ram:0,bsram:0,owner:"U/V4 each; low/high W edges",status:"finite transport replay"},
        Added{name:"wrapped U/V",ff:0,ram:18,bsram:0,owner:"32x36 two depth banks; ingress1W / coordinate1R",status:"finite transport replay"},
        Added{name:"wrapped return FF",ff:36,ram:0,bsram:0,owner:"registered1-cycle read response before child capture",status:"finite transport replay"},
        Added{name:"quad metadata",ff:0,ram:3,bsram:0,owner:"8x12 qid/mask/slot; ingress W / cohort R",status:"finite transport replay"},
        Added{name:"LOD consumers",ff:0,ram:18,bsram:0,owner:"8x71; LOD W / cohort R; no second coefficient R",status:"finite transport replay"},
        Added{name:"cohort/metadata return bank",ff:83,ram:0,bsram:0,owner:"header12+LOD71; return bank promoted to held cohort; direct LOD only at ready edge",status:"one owned cohort finite replay"},
        Added{name:"D handoff",ff:40,ram:0,bsram:0,owner:"one registered slope handoff to LOD",status:"owner/value read checked"},
        Added{name:"LOD handoff",ff:71,ram:0,bsram:0,owner:"one71-bit W/first lane bypass cut",status:"owner/value/ready read checked"},
        Added{name:"source adapter control",ff:35,ram:0,bsram:0,owner:"ingress7 / raw R7 / cohort7 / Dcut4 / LODcut4 / wrapped reply6; retains original prep control too",status:"conservative separate fields"},
        Added{name:"lane geometry bank",ff:0,ram:25,bsram:0,owner:"16x99; coordinate W / coefficient then member R in disjoint phases",status:"PLANNED calendar; not end-to-end replay"},
        Added{name:"lane fraction/weight bank",ff:0,ram:18,bsram:0,owner:"16x72; coordinate writes51 useful bits then coefficient replaces by72 weights; one R/W",status:"PLANNED calendar; not end-to-end replay"},
        Added{name:"lane index FIFOs",ff:0,ram:2,bsram:0,owner:"two16x4 banks; keep logical coordinate credit6 and coefficient-ready credit2",status:"PLANNED finite ready adapters"},
        Added{name:"lane head",ff:171,ram:0,bsram:0,owner:"capture99+72 once; fine/coarse member consume on two edges",status:"PLANNED port/capture calendar"},
        Added{name:"lane row control",ff:13,ram:0,bsram:0,owner:"two4-bit pointers+count5; original prep control retained",status:"PLANNED conservative control"},
        Added{name:"plane head operands",ff:92,ram:0,bsram:0,owner:"existing cursor3 FF remains; snapshot92 once from16x94 work RAM",status:"PLANNED synchronous head adapter"},
        Added{name:"packet payload",ff:0,ram:0,bsram:2,owner:"64 used rows2x512x36; sole packet W / sole cache head R",status:"finite synchronous port/CE/fault/wrap replay"},
        Added{name:"producer indices",ff:0,ram:2,bsram:0,owner:"16x6; issued+ready producer credits<=16",status:"finite ownership replay; physical SSRAM mux pending"},
        Added{name:"Group indices",ff:0,ram:4,bsram:0,owner:"32x6; queue+pending+head<=32",status:"finite ownership replay; physical SSRAM mux pending"},
        Added{name:"packet head",ff:73,ram:0,bsram:0,owner:"72-bit skid+valid; capture frees row; cache consume frees Group credit",status:"finite synchronous replay; hot packet II2"},
        Added{name:"packet index in pipeline",ff:56,ram:0,bsram:0,owner:"8x(index6+valid1); W address must not be free",status:"conservative fixed8-cycle reservation"},
        Added{name:"packet pending R",ff:7,ram:0,bsram:0,owner:"row6+valid1; macro BP72 retained by common CE/OCE",status:"finite synchronous replay"},
        Added{name:"packet ring control",ff:19,ram:0,bsram:0,owner:"alloc6/reclaim6/count7",status:"finite ordered-ring replay"},
        Added{name:"packet index queue control",ff:29,ram:0,bsram:0,owner:"producer ptr4+4/count5; Group ptr5+5/count6; old cache control also retained",status:"conservative separate control"},
        Added{name:"slot effective fields",ff:0,ram:10,bsram:0,owner:"16x38; drained writes; validate/lookup R time-shared",status:"PLANNED slot port controller"},
        Added{name:"immutable active draw",ff:60,ram:0,bsram:0,owner:"base32/max4/has1/valid1/slot4/filter2/bias16; drained rebind",status:"PLANNED protected draw reference"},
        Added{name:"color weights",ff:72,ram:0,bsram:0,owner:"two36-bit operand banks; consumed only at DSP capture",status:"independent integer and owner replay"},
        Added{name:"color texel return",ff:64,ram:0,bsram:0,owner:"one4x16 bank; dead after DSP capture",status:"independent integer and owner replay"},
        Added{name:"color read line",ff:6,ram:0,bsram:0,owner:"read reservation through1-cycle texel return",status:"existing cache trace input; conservative latch"},
        Added{name:"color add tree",ff:153,ram:0,bsram:0,owner:"first sums102 / partial51",status:"independent integer and owner replay"},
        Added{name:"color feedback",ff:58,ram:0,bsram:0,owner:"sum51/key6/valid1; retained across misses; last sum read before next write",status:"independent integer and owner replay"},
        Added{name:"color normalize cuts",ff:81,ram:0,bsram:0,owner:"fold54 / increment27; three plain registered logic dependencies",status:"independent integer and owner replay"},
        Added{name:"color RGB output",ff:24,ram:0,bsram:0,owner:"one24-bit output before actual public W",status:"independent integer and owner replay"},
        Added{name:"color identity",ff:66,ram:0,bsram:0,owner:"11xkey6",status:"fixed pipeline owner replay"},
        Added{name:"color first/last",ff:18,ram:0,bsram:0,owner:"first7 / last11",status:"fixed pipeline flag replay"},
        Added{name:"color pipeline valid",ff:11,ram:0,bsram:0,owner:"11 valid positions; CE-gated together",status:"bounded replay"},
        Added{name:"post-write done",ff:7,ram:0,bsram:0,owner:"key6+valid; only after actual public result W",status:"bounded replay; global release at final capture"},
    ];
    for row in added {
        writeln!(
            out,
            "{},0,{},0,{},0,{},{},{}",
            csv(row.name),
            row.ff,
            row.ram,
            row.bsram,
            csv(row.status),
            csv(row.owner)
        )
        .unwrap();
        ff += row.ff;
        ram += row.ram;
        bsram += row.bsram;
    }
    assert_eq!((ff, ram, bsram), (6695, 156, 8));
    writeln!(
        out,
        "TOTAL,{}, {},{}, {},{}, {},provisional single candidate,not fitted or composed",
        old.ff_bits, ff, old.sdp4_cells, ram, old.bsram, bsram
    )
    .unwrap();
    println!("one candidate ledger: old FF={} SDP4={} BSRAM={} -> provisional FF={ff} SDP4={ram} BSRAM={bsram}; one public result BSRAM outside sampling; hard DSP bits={}",old.ff_bits,old.sdp4_cells,old.bsram,old.hard_pipeline_bits);
}
