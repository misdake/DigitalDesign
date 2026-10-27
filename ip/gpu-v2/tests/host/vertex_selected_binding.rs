//! Small binding test; the existing full source-matched matrix is reused.
use gpu_v2::vertex::{
    do_loan, do_loan_reference,
    emu::{Header, Inputs, Ticket},
    ports::PackedVertex,
    rtl,
};
#[test]
fn selected_default_matches_qualified_source_and_reference_result() {
    assert_eq!(gpu_v2::vertex::ENABLED_EDGES, 90);
    for (selected, qualified) in [
        (rtl::do_loan_portable(), rtl::single_edge_portable()),
        (rtl::do_loan_native(), rtl::single_edge_native()),
    ] {
        assert_eq!(
            selected.replace(
                "module gpu_v2_vertex_do_loan(",
                "module gpu_v2_vertex_single_edge("
            ),
            qualified
        );
    }
    let mut new = do_loan::VertexEmu::default();
    let mut old = do_loan_reference::VertexEmu::default();
    let start = Inputs {
        ce: true,
        config: Some(Header::default()),
        ..Inputs::default()
    };
    new.tick(start);
    old.tick(start);
    for i in 0..25 {
        let coefficient = if i < 16 && i % 5 == 0 {
            65536
        } else if i >= 16 && (i - 16) % 4 == 0 {
            16384
        } else {
            0
        };
        let input = Inputs {
            ce: true,
            coefficient: Some(coefficient),
            ..Inputs::default()
        };
        new.tick(input);
        old.tick(input);
    }
    let v = PackedVertex::encode([0, 0, 0], [0, 0, 127], [123, 4095], 0x1234).unwrap();
    let input = Inputs {
        ce: true,
        vertex: Some((
            v,
            Ticket {
                slot: true,
                epoch: 17,
                vertex: 3,
            },
        )),
        write_ready: true,
        publish_ready: true,
        ..Inputs::default()
    };
    new.tick(input);
    old.tick(input);
    let mut nr = vec![];
    let mut or = vec![];
    let mut ne = None;
    let mut oe = None;
    for wall in 1..2000 {
        let input = Inputs {
            ce: true,
            write_ready: true,
            publish_ready: true,
            ..Inputs::default()
        };
        let n = new.outputs(&input);
        let o = old.outputs(&input);
        if n.write_valid {
            nr.push(n.row_data);
        }
        if o.write_valid {
            or.push(o.row_data);
        }
        if n.publish_valid {
            ne = Some(wall);
            assert_eq!(n.ticket, input_ticket());
        }
        if o.publish_valid {
            oe = Some(wall);
            assert_eq!(o.ticket, input_ticket());
        }
        new.tick(input);
        old.tick(input);
        if ne.is_some() && oe.is_some() {
            break;
        }
    }
    assert_eq!(nr, or);
    assert_eq!(nr.len(), 7);
    assert_eq!(oe.unwrap() - ne.unwrap(), 12);
}
fn input_ticket() -> Ticket {
    Ticket {
        slot: true,
        epoch: 17,
        vertex: 3,
    }
}
