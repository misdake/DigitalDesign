//! Real Work16/short Packet/Pool64 composition versus current Runtime edges.
use super::super::compact_transport;
use crate::texture::emu::packet_store_ordered::qualification::run;
use crate::texture::emu::packet_store_ordered::{Control, Edge, Tick};
use std::{fmt::Write, path::Path};

pub(super) struct Replay {
    tb: String,
    edges: usize,
}
impl Replay {
    pub(super) fn new() -> Self {
        Self {tb:String::from("module tb;reg clk=0,reset=0,ce=0,abort=0,issue_enable=1,in_valid=0,out_ready=0;reg[75:0]in_member=0;wire in_ready,out_valid,fault,capture_fire,work_ack;wire[71:0]out_payload;wire[5:0]pool_strobes,group_count,row_count;wire[4:0]producer_count,unwritten_count,work_count;\n`ifdef NATIVE\nGSR GSR(.GSRI(1'b1));\n`endif\ngpu_v2_texture_runtime_packet_backend dut(.*);initial begin #8000000;$fatal(1,\"backend watchdog\");end initial begin\n"),edges:0}
    }
    // Explicit old/new observations from distinct real components are test
    // inputs, never a combined retained hardware state or a production API.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sample(
        &mut self,
        t: Tick,
        e: Edge,
        work_input: Option<compact_transport::Member>,
        issue_enable: bool,
        before: Option<u128>,
        after: (Control, Option<u128>, bool, u128),
        work: compact_transport::Snapshot,
        banks: [Option<u128>; 9],
    ) {
        let b = |v| u8::from(v);
        writeln!(
            self.tb,
            "reset={};ce={};in_valid={};in_member=76'h{:019x};issue_enable={};out_ready={};#1;",
            b(t.reset),
            b(t.ce),
            b(work_input.is_some()),
            work_input.map_or(0, |m| m.bits()),
            b(issue_enable),
            b(t.consume)
        )
        .unwrap();
        let strobes = if t.ce && !t.reset {
            b(e.reserve.is_some())
                | b(e.write.is_some()) << 1
                | b(e.transfer.is_some()) << 2
                | b(e.read.is_some()) << 3
                | b(e.capture.is_some()) << 4
                | b(e.consume.is_some()) << 5
        } else {
            0
        };
        writeln!(self.tb,"if(fault || pool_strobes!==6'd{} || capture_fire!==1'b{})$fatal(1,\"backend old ports {}\");",strobes,b(t.ce&&!t.reset&&e.reserve.is_some()),self.edges).unwrap();
        if !t.reset {
            writeln!(
                self.tb,
                "if(out_valid!==1'b{})$fatal(1,\"backend old valid {}\");",
                b(before.is_some()),
                self.edges
            )
            .unwrap();
            if let Some(w) = before {
                writeln!(
                    self.tb,
                    "if(out_payload!==72'h{w:018x})$fatal(1,\"backend old payload {}\");",
                    self.edges
                )
                .unwrap();
            }
            if work_input.is_some() && t.ce {
                self.tb
                    .push_str("if(!in_ready)$fatal(1,\"actual Work write denied\");\n");
            }
        }
        self.tb.push_str("clk=1;#1;\n");
        let (c, front, fault, hard) = after;
        assert!(!fault);
        for (name, v) in [
            ("alloc", u64::from(c.allocate)),
            ("wptr", c.write.into()),
            ("tptr", c.transfer.into()),
            ("rptr", c.read.into()),
            ("reclaim", c.reclaim.into()),
            ("pc", c.producer.into()),
            ("gc", c.groups.into()),
            ("rows", c.rows.into()),
            ("uw", c.unwritten.into()),
            ("hv", (b(c.head_valid[0]) | b(c.head_valid[1]) << 1).into()),
            ("rh", c.read_head.into()),
            ("ph", c.pop_head.into()),
            ("pending", b(c.pending).into()),
            ("prow", c.pending_row.into()),
            ("phead", c.pending_head.into()),
        ] {
            writeln!(
                self.tb,
                "if(dut.u_pool.{name}!=={v})$fatal(1,\"backend pool {name} {}\");",
                self.edges
            )
            .unwrap();
        }
        for (name, v) in [
            ("count", work.materialized),
            ("head_count", work.loaded),
            ("fetch_ptr", work.fetch),
            ("write_ptr", work.write),
            ("reclaim_ptr", work.read),
            ("head_valid", work.head_valid as usize),
            ("consume_head", work.consume_head),
            ("fill_head", work.fill_head),
        ] {
            writeln!(
                self.tb,
                "if(dut.u_work.{name}!=={v})$fatal(1,\"backend Work {name} {}\");",
                self.edges
            )
            .unwrap();
        }
        for h in 0..2 {
            if work.head_valid >> h & 1 != 0 {
                writeln!(self.tb,"if(dut.u_work.head{h}!==76'h{:019x} || dut.u_work.cursors[{h}]!=={})$fatal(1,\"backend Work owned head {}\");",work.heads[h].bits(),work.cursors[h],self.edges).unwrap();
            }
        }
        // Compact debug banks preserve the historical age2 index for word0.
        for (i, bank) in [banks[0], banks[2]].iter().enumerate() {
            writeln!(
                self.tb,
                "if(dut.u_packet.valid[{i}]!==1'b{})$fatal(1,\"actual Packet valid {}\");",
                b(bank.is_some()),
                self.edges
            )
            .unwrap();
            if let Some(w) = bank {
                let name = if i == 0 { "capture0" } else { "word0" };
                writeln!(
                    self.tb,
                    "if(dut.u_packet.{name}!==77'h{w:020x})$fatal(1,\"actual Packet bank {}\");",
                    self.edges
                )
                .unwrap();
            }
        }
        writeln!(self.tb,"if(dut.returned_payload!==72'h{hard:018x})$fatal(1,\"backend paid DO every wall {}\");",self.edges).unwrap();
        if let Some(w) = front {
            writeln!(
                self.tb,
                "if(!out_valid || out_payload!==72'h{w:018x})$fatal(1,\"backend new front {}\");",
                self.edges
            )
            .unwrap();
        }
        self.tb.push_str("clk=0;#1;\n");
        self.edges += 1;
    }
    pub(super) fn finish(mut self, path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        self.tb
            .push_str("$display(\"PASS packet store actual backend\");$finish;end endmodule\n");
        std::fs::write(path.join("tb.v"), &self.tb).unwrap();
        for (name, native) in [("portable", false), ("native", true)] {
            std::fs::write(
                path.join(format!("{name}.v")),
                crate::texture::rtl::runtime_packet_backend::source(native),
            )
            .unwrap();
        }
        // Keep the matched generic-owner baseline available for replay/fit.
        std::fs::write(
            path.join("reference-native.v"),
            crate::texture::rtl::runtime_packet_backend::reference_source(true),
        )
        .unwrap();
        std::fs::write(
            path.join("reference-portable.v"),
            crate::texture::rtl::runtime_packet_backend::reference_source(false),
        )
        .unwrap();
        std::fs::write(path.join("receipt.json"),format!("{{\"edges\":{},\"Work\":16,\"Producer\":16,\"Group\":32,\"Pool\":64,\"scope\":\"actual compact Runtime Work/Packet/Pool combination ports, banks and paid DO\"}}",self.edges)).unwrap();
    }
    pub(super) fn run(path: &Path) {
        run(path);
    }
}

#[test]
fn selected_and_reference_sources_match_the_qualified_current_calendars() {
    let Some(root) = std::env::var_os("GPU_ORDERED_PACKET_STORAGE_OUTPUT") else {
        return; // Artifact identity check is opt-in; ordinary builds need no files.
    };
    // Model-only qualification exports the same actual backend sources/TBs;
    // selecting that directory does not claim they have been RTL replayed.
    let kind =
        std::env::var("GPU_ORDERED_PACKET_CALENDAR_KIND").unwrap_or_else(|_| "runtime-rtl".into());
    assert!(kind == "runtime" || kind == "runtime-rtl");
    let path = std::path::PathBuf::from(root).join(kind);
    let mut calendars = 0;
    for entry in std::fs::read_dir(path).unwrap() {
        let directory = entry.unwrap().path();
        if !directory.is_dir()
            || !directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-backend")
        {
            continue;
        }
        for (name, native) in [("portable", false), ("native", true)] {
            assert_eq!(
                std::fs::read_to_string(directory.join(format!("{name}.v"))).unwrap(),
                crate::texture::rtl::runtime_packet_backend::source(native)
            );
            assert_eq!(
                std::fs::read_to_string(directory.join(format!("reference-{name}.v"))).unwrap(),
                crate::texture::rtl::runtime_packet_backend::reference_source(native)
            );
        }
        calendars += 1;
    }
    assert_eq!(calendars, 84);
}

#[test]
#[ignore = "requires generated current84 model calendars, Icarus and Gowin primitives"]
fn one_default_backend_calendar_preserves_actual_key_ports() {
    let root = std::env::var_os("GPU_ORDERED_PACKET_STORAGE_OUTPUT")
        .expect("generate packet_storage_runtime_84_baseline_calendars first");
    Replay::run(&std::path::PathBuf::from(root).join("runtime/N8-C6-Etrue-M0-P0-backend"));
}
