//! Bounded live subprocess co-simulation. No frontend traces/plans execute here.
use gpu_v2::command_processor::{control::*, emu::CommandEmu, ports::Command, rtl};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command as Process, Stdio},
    sync::mpsc,
    time::Duration,
};

fn pack_input(i: Inputs) -> u64 {
    let r = i.command.unwrap_or_default();
    u64::from(i.reset)
        | u64::from(i.ce) << 1
        | u64::from(i.command.is_some()) << 2
        | u64::from(r.kinds) << 3
        | u64::from(r.token) << 9
        | u64::from(r.validated) << 12
        | u64::from(i.event_set) << 13
        | u64::from(i.event_ack) << 21
        | u64::from(i.dma_done) << 29
        | u64::from(i.dma_ready) << 33
        | u64::from(i.draw_ready) << 34
        | u64::from(i.draw_done) << 35
        | u64::from(i.release_ready) << 36
        | u64::from(i.fence_ready) << 37
        | u64::from(i.external_fault) << 38
}
fn pack_output(o: Outputs) -> u32 {
    u32::from(o.command_capture)
        | u32::from(o.command_done) << 1
        | u32::from(o.handler.is_some()) << 2
        | u32::from(o.handler.unwrap_or(0)) << 3
        | u32::from(o.dma_valid) << 6
        | u32::from(o.dma_accept) << 7
        | u32::from(o.dma_token) << 8
        | u32::from(o.wait_ack) << 10
        | u32::from(o.draw_valid) << 11
        | u32::from(o.draw_accept) << 12
        | u32::from(o.release_valid) << 13
        | u32::from(o.release_accept) << 14
        | u32::from(o.fence_ack) << 15
        | u32::from(o.fault) << 16
}
fn pack_state(s: State) -> u32 {
    u32::from(s.phase.code())
        | u32::from(s.token) << 3
        | u32::from(s.tokens) << 6
        | u32::from(s.reserved) << 10
        | u32::from(s.pending) << 14
        | u32::from(s.cursor) << 22
        | u32::from(s.draw_return) << 25
}
const TB: &str = r#"
module tb;
 reg clk=0;reg[63:0]stimulus;integer got,edges=0;
 wire command_capture,command_done,handler_valid,dma_valid,dma_accept,wait_ack,draw_valid,draw_accept,release_valid,release_accept,fence_ack,fault;
 wire[2:0]handler_id,phase_state,token_state,cursor_state;wire[1:0]dma_token;wire[3:0]tokens_state,reserved_state;wire[7:0]pending_state;wire draw_return_state;
 reg[16:0]observed;
 gpu_v2_command_control dut(.clk(clk),.reset(stimulus[0]),.ce(stimulus[1]),.command_valid(stimulus[2]),
 .command_dma(stimulus[3]),.command_wait(stimulus[4]),.command_draw(stimulus[5]),.command_release(stimulus[6]),.command_fence(stimulus[7]),.command_unsupported(stimulus[8]),
 .command_token(stimulus[11:9]),.command_validated(stimulus[12]),.event_set(stimulus[20:13]),.event_ack(stimulus[28:21]),.dma_done(stimulus[32:29]),
 .dma_ready(stimulus[33]),.draw_ready(stimulus[34]),.draw_done(stimulus[35]),.release_ready(stimulus[36]),.fence_ready(stimulus[37]),.external_fault(stimulus[38]),
 .command_capture(command_capture),.command_done(command_done),.handler_valid(handler_valid),.handler_id(handler_id),.dma_valid(dma_valid),.dma_accept(dma_accept),.dma_token(dma_token),
 .wait_ack(wait_ack),.draw_valid(draw_valid),.draw_accept(draw_accept),.release_valid(release_valid),.release_accept(release_accept),.fence_ack(fence_ack),.fault(fault),
 .phase_state(phase_state),.token_state(token_state),.tokens_state(tokens_state),.reserved_state(reserved_state),.pending_state(pending_state),.cursor_state(cursor_state),.draw_return_state(draw_return_state));
 initial begin
  while(edges<10000)begin
   got=$fscanf(32'h80000000,"%h",stimulus);
   if(got!=1)$finish;
   #1;observed={fault,fence_ack,release_accept,release_valid,draw_accept,draw_valid,wait_ack,dma_token,dma_accept,dma_valid,(handler_valid?handler_id:3'b0),handler_valid,command_done,command_capture};
   clk=1;#1;clk=0;
   $display("ROW %h %h",observed,{draw_return_state,cursor_state,pending_state,reserved_state,tokens_state,token_state,phase_state});$fflush();
   edges=edges+1;
  end
  $fatal(1,"bounded live command cosim exceeded10000edges");
 end
endmodule
"#;
struct Live {
    child: Child,
    input: ChildStdin,
    rows: mpsc::Receiver<String>,
    log: fs::File,
    model: CommandEmu,
    edges: u32,
    states: u8,
    captures: u32,
    done: u32,
    ce0: u32,
    handlers: [u32; 8],
}
impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Live {
    fn new(root: &Path, backend: &str) -> Self {
        let out = root.join("live").join(backend);
        fs::create_dir_all(&out).unwrap();
        let source = root.join("source").join(backend).join("command-control.v");
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            rtl::verilog(),
            "actual exported source drift"
        );
        fs::write(out.join("tb.v"), TB).unwrap();
        let compile =
            Process::new(std::env::var("IVERILOG_EXE").unwrap_or_else(|_| "iverilog".into()))
                .args(["-g2012", "-s", "tb", "-o", "sim.vvp"])
                .arg(&source)
                .arg("tb.v")
                .current_dir(&out)
                .output()
                .unwrap();
        fs::write(out.join("compile.err"), &compile.stderr).unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let mut child = Process::new(std::env::var("VVP_EXE").unwrap_or_else(|_| "vvp".into()))
            .arg("sim.vvp")
            .current_dir(&out)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rows) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if tx.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            rows,
            log: fs::File::create(out.join("live.csv")).unwrap(),
            model: CommandEmu::default(),
            edges: 0,
            states: 0,
            captures: 0,
            done: 0,
            ce0: 0,
            handlers: [0; 8],
        }
    }
    fn tick(&mut self, i: Inputs) -> Outputs {
        assert!(self.edges < 9000, "bounded host edge count");
        let stimulus = pack_input(i);
        writeln!(self.input, "{stimulus:016x}").unwrap();
        self.input.flush().unwrap();
        let expected = self.model.step(i);
        let line = self
            .rows
            .recv_timeout(Duration::from_secs(5))
            .expect("bounded actual RTL response");
        let mut fields = line.split_whitespace();
        assert_eq!(fields.next(), Some("ROW"), "{line}");
        let actual =
            u32::from_str_radix(fields.next().unwrap(), 16).expect("actual output contains X");
        let actual_state =
            u32::from_str_radix(fields.next().unwrap(), 16).expect("actual state contains X");
        assert_eq!(
            actual,
            pack_output(expected),
            "live output edge{} input{stimulus:x}",
            self.edges
        );
        assert_eq!(
            actual_state,
            pack_state(self.model.state),
            "live state edge{} input{stimulus:x}",
            self.edges
        );
        writeln!(
            self.log,
            "{},{stimulus:x},{actual:x},{actual_state:x}",
            self.edges
        )
        .unwrap();
        self.edges += 1;
        self.states |= 1 << self.model.state.phase.code();
        self.ce0 += u32::from(!i.ce);
        self.captures += u32::from(expected.command_capture);
        self.done += u32::from(expected.command_done);
        if let Some(id) = expected.handler {
            assert!(id != 5 && id != 6);
            self.handlers[usize::from(id)] += 1;
        }
        expected
    }
    fn reset(&mut self) {
        self.tick(Inputs {
            reset: true,
            ..Inputs::default()
        });
    }
    fn capture(&mut self, kinds: u8, token: u8) {
        let request = Request {
            kinds,
            token,
            validated: true,
        };
        for _ in 0..16 {
            if self
                .tick(Inputs {
                    ce: true,
                    command: Some(request),
                    ..Inputs::default()
                })
                .command_capture
            {
                return;
            }
        }
        panic!("bounded command capture");
    }
}
#[test]
fn preparation_preserves_existing_typed_guards() {
    use gpu_v2::{scratchpad::ports::DmaDescriptor, vertex::ports::Context};
    let commands = [
        Command::Wait(4),
        Command::Unsupported(99),
        Command::Release { slot: 0, epoch: 0 },
        Command::Release { slot: 2, epoch: 1 },
        Command::Dma(DmaDescriptor {
            physical_addr: u64::MAX - 7,
            scratchpad_addr: 0,
            byte_count: 8,
            completion_token: 0,
        }),
        Command::Dma(DmaDescriptor {
            physical_addr: 1,
            scratchpad_addr: 0,
            byte_count: 8,
            completion_token: 0,
        }),
        Command::Dma(DmaDescriptor {
            physical_addr: 0,
            scratchpad_addr: 4088,
            byte_count: 16,
            completion_token: 0,
        }),
        Command::Draw {
            region: 0,
            byte_offset: 4092,
            vertices: 1,
            context: Context::default(),
        },
        Command::Draw {
            region: 0,
            byte_offset: 0,
            vertices: 3,
            context: Context {
                normal_matrix: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
                ..Context::default()
            },
        },
    ];
    for c in commands {
        assert!(PreparedCommand::new(c).is_err());
    }
    for c in [
        Command::Wait(3),
        Command::Fence,
        Command::Release { slot: 1, epoch: 1 },
        Command::Draw {
            region: 1,
            byte_offset: 0,
            vertices: 64,
            context: Context::default(),
        },
    ] {
        assert!(PreparedCommand::new(c).is_ok());
    }
}
#[test]
fn events_match_existing_audit_without_auto_handler_ack() {
    use gpu_v2::command_processor::sim::timed::{audit_events, EventTick};
    let mut model = CommandEmu::default();
    let mut trace = Vec::new();
    for cycle in 0..1024u64 {
        let set = ((cycle * 73 + 19) & 255) as u8;
        let ack = ((cycle * 31 + 7) & 255) as u8;
        let ce = cycle % 5 != 0;
        let o = model.step(Inputs {
            ce,
            event_set: set,
            event_ack: ack,
            ..Inputs::default()
        });
        trace.push(EventTick {
            cycle,
            set,
            ack: if ce { ack } else { 0 },
            boundary: ce,
            handler: o.handler,
            pending: model.state.pending,
        });
    }
    audit_events(&trace, 2048).unwrap();
}
#[test]
#[ignore = "requires actual Icarus subprocess and isolated output directory"]
fn command_kernel_live_cosim() {
    let root = PathBuf::from(
        std::env::var("GPU_COMMAND_QUAL_DIR").expect("explicit isolated output directory"),
    );
    for backend in ["native", "portable"] {
        let mut t = Live::new(&root, backend);
        t.reset();
        // Event selection uses current set before selection, explicit ack only.
        t.tick(Inputs {
            event_set: 255,
            event_ack: 255,
            ..Inputs::default()
        });
        assert_eq!(t.model.state.pending, 255);
        for wanted in [0, 1, 2, 3, 4, 7, 0, 1] {
            let o = t.tick(Inputs {
                ce: true,
                ..Inputs::default()
            });
            assert_eq!(o.handler, Some(wanted));
            assert_eq!(t.model.state.pending, 255);
        }
        t.tick(Inputs {
            ce: true,
            event_set: 1,
            event_ack: 1,
            ..Inputs::default()
        });
        assert_eq!(t.model.state.pending, 255);
        t.tick(Inputs {
            ce: true,
            event_ack: 255,
            ..Inputs::default()
        });
        assert_eq!(t.model.state.pending, 0);
        for token in 0..4 {
            t.reset();
            t.capture(1, token);
            for _ in 0..3 {
                let o = t.tick(Inputs {
                    ce: true,
                    command: Some(Request {
                        kinds: 16,
                        token: 3,
                        validated: true,
                    }),
                    ..Inputs::default()
                });
                assert!(o.dma_valid && !o.dma_accept && o.dma_token == token);
                assert_eq!(t.model.state.reserved, 0);
            }
            assert!(
                !t.tick(Inputs {
                    dma_ready: true,
                    ..Inputs::default()
                })
                .dma_accept
            );
            assert!(
                t.tick(Inputs {
                    ce: true,
                    dma_ready: true,
                    ..Inputs::default()
                })
                .dma_accept
            );
            assert_eq!(t.model.state.reserved, 1 << token);
            t.capture(2, token);
            assert!(
                !t.tick(Inputs {
                    ce: true,
                    ..Inputs::default()
                })
                .wait_ack
            );
            // A same-wall completion can satisfy WAIT, while event set wins.
            assert!(
                t.tick(Inputs {
                    ce: true,
                    dma_done: 1 << token,
                    event_ack: 1 << token,
                    ..Inputs::default()
                })
                .wait_ack
            );
            assert_eq!(t.model.state.tokens, 0);
            assert_eq!(t.model.state.reserved, 0);
            assert_ne!(t.model.state.pending & (1 << token), 0);
            t.capture(1, token);
            assert!(
                t.tick(Inputs {
                    ce: true,
                    dma_ready: true,
                    ..Inputs::default()
                })
                .dma_accept
            );
            t.tick(Inputs {
                dma_done: 1 << token,
                event_ack: 255,
                ..Inputs::default()
            });
            assert_eq!(t.model.state.tokens, 1 << token);
            t.capture(2, token);
            assert!(!t.tick(Inputs::default()).wait_ack);
            assert!(
                t.tick(Inputs {
                    ce: true,
                    ..Inputs::default()
                })
                .wait_ack
            );
            // Token reuse before WAIT is atomically rejected, even under ready.
            t.capture(1, token);
            t.tick(Inputs {
                ce: true,
                dma_ready: true,
                ..Inputs::default()
            });
            let o = t.tick(Inputs {
                ce: true,
                dma_ready: true,
                command: Some(Request {
                    kinds: 1,
                    token,
                    validated: true,
                }),
                ..Inputs::default()
            });
            assert!(o.fault && !o.command_capture && !o.dma_accept);
            assert_eq!(t.model.state.reserved, 1 << token);
        }
        for pause in [false, true] {
            t.reset();
            t.capture(4, 0);
            for _ in 0..4 {
                assert!(
                    t.tick(Inputs {
                        ce: true,
                        ..Inputs::default()
                    })
                    .draw_valid
                );
            }
            assert!(
                !t.tick(Inputs {
                    draw_ready: true,
                    ..Inputs::default()
                })
                .draw_accept
            );
            assert!(
                t.tick(Inputs {
                    ce: true,
                    draw_ready: true,
                    ..Inputs::default()
                })
                .draw_accept
            );
            assert!(
                !t.tick(Inputs {
                    ce: !pause,
                    draw_done: true,
                    ..Inputs::default()
                })
                .command_done
            );
            assert!(t.model.state.draw_return);
            if pause {
                t.tick(Inputs::default());
                assert!(t.model.state.draw_return);
            }
            assert!(
                t.tick(Inputs {
                    ce: true,
                    ..Inputs::default()
                })
                .command_done
            );
        }
        t.reset();
        t.capture(4, 0);
        let o = t.tick(Inputs {
            ce: true,
            draw_ready: true,
            draw_done: true,
            ..Inputs::default()
        });
        assert!(o.fault && !o.draw_accept && !o.command_done);
        for kinds in [8, 16] {
            t.reset();
            t.capture(kinds, 0);
            for _ in 0..4 {
                assert!(
                    !t.tick(Inputs {
                        ce: true,
                        ..Inputs::default()
                    })
                    .command_done
                );
            }
            assert!(
                !t.tick(Inputs {
                    release_ready: true,
                    fence_ready: true,
                    ..Inputs::default()
                })
                .command_done
            );
            assert!(
                t.tick(Inputs {
                    ce: true,
                    release_ready: true,
                    fence_ready: true,
                    ..Inputs::default()
                })
                .command_done
            );
            t.capture(kinds, 0);
            let o = t.tick(Inputs {
                ce: true,
                release_ready: true,
                fence_ready: true,
                external_fault: true,
                ..Inputs::default()
            });
            assert!(o.fault && !o.command_done && !o.release_accept);
        }
        for request in [
            Request {
                kinds: 0,
                token: 0,
                validated: true,
            },
            Request {
                kinds: 3,
                token: 0,
                validated: true,
            },
            Request {
                kinds: 32,
                token: 0,
                validated: true,
            },
            Request {
                kinds: 1,
                token: 4,
                validated: true,
            },
            Request {
                kinds: 2,
                token: 4,
                validated: true,
            },
            Request {
                kinds: 4,
                token: 0,
                validated: false,
            },
        ] {
            t.reset();
            let before = t.model.state;
            t.tick(Inputs {
                command: Some(request),
                ..Inputs::default()
            });
            assert_eq!(t.model.state.phase, before.phase);
            let o = t.tick(Inputs {
                ce: true,
                command: Some(request),
                dma_ready: true,
                draw_ready: true,
                release_ready: true,
                fence_ready: true,
                ..Inputs::default()
            });
            assert!(o.fault && !o.command_capture && !o.command_done);
            assert_eq!(t.model.state.reserved, 0);
            assert_eq!(t.model.state.tokens, 0);
            t.tick(Inputs {
                ce: true,
                ..Inputs::default()
            });
            assert_eq!(t.model.state.phase, Phase::Fault);
        }
        // The real checked adapter, not a caller-managed payload-lifetime promise.
        t.reset();
        let mut port = CommandPort::default();
        let descriptor = gpu_v2::scratchpad::ports::DmaDescriptor {
            physical_addr: 0x1000,
            scratchpad_addr: 0,
            byte_count: 40,
            completion_token: 0,
        };
        port.present(Command::Dma(descriptor)).unwrap();
        for _ in 0..16 {
            let o = t.tick(Inputs {
                ce: true,
                command: port.request(),
                ..Inputs::default()
            });
            port.edge(o).unwrap();
            if o.command_capture {
                break;
            }
        }
        assert!(port.request().is_none());
        assert!(port.payload().is_some());
        assert!(port.present(Command::Fence).is_err());
        for ce in [true, false, true] {
            let o = t.tick(Inputs {
                ce,
                ..Inputs::default()
            });
            assert!(!o.dma_accept);
            assert!(port.edge(o).unwrap().is_none());
            assert!(matches!(port.payload(),Some(Command::Dma(d)) if *d==descriptor));
        }
        let o = t.tick(Inputs {
            ce: true,
            dma_ready: true,
            ..Inputs::default()
        });
        assert!(matches!(port.edge(o).unwrap(),Some(Command::Dma(d)) if d==descriptor));
        port.present(Command::Wait(0)).unwrap();
        for _ in 0..16 {
            let o = t.tick(Inputs {
                ce: true,
                command: port.request(),
                ..Inputs::default()
            });
            port.edge(o).unwrap();
            if o.command_capture {
                break;
            }
        }
        let o = t.tick(Inputs {
            dma_done: 1,
            ..Inputs::default()
        });
        assert!(port.edge(o).unwrap().is_none());
        let o = t.tick(Inputs {
            ce: true,
            ..Inputs::default()
        });
        assert!(matches!(port.edge(o).unwrap(), Some(Command::Wait(0))));
        let context = gpu_v2::vertex::ports::Context::default();
        port.present(Command::Draw {
            region: 0,
            byte_offset: 4,
            vertices: 3,
            context: context.clone(),
        })
        .unwrap();
        for _ in 0..16 {
            let o = t.tick(Inputs {
                ce: true,
                command: port.request(),
                ..Inputs::default()
            });
            port.edge(o).unwrap();
            if o.command_capture {
                break;
            }
        }
        for ce in [true, false, true] {
            let o = t.tick(Inputs {
                ce,
                ..Inputs::default()
            });
            assert!(port.edge(o).unwrap().is_none());
            assert!(matches!(
                port.payload(),
                Some(Command::Draw {
                    byte_offset: 4,
                    vertices: 3,
                    ..
                })
            ));
        }
        let o = t.tick(Inputs {
            ce: true,
            draw_ready: true,
            ..Inputs::default()
        });
        assert!(
            matches!(port.edge(o).unwrap(),Some(Command::Draw {region:0,byte_offset:4,vertices:3,context:c}) if c.mvp==context.mvp&&c.normal_matrix==context.normal_matrix)
        );
        assert!(port.payload().is_none());
        assert!(port.present(Command::Fence).is_err());
        let o = t.tick(Inputs {
            draw_done: true,
            ..Inputs::default()
        });
        port.edge(o).unwrap();
        let o = t.tick(Inputs {
            ce: true,
            ..Inputs::default()
        });
        assert!(o.command_done);
        port.edge(o).unwrap();
        port.present(Command::Fence).unwrap();
        for _ in 0..16 {
            let o = t.tick(Inputs {
                ce: true,
                command: port.request(),
                ..Inputs::default()
            });
            port.edge(o).unwrap();
            if o.command_capture {
                break;
            }
        }
        let o = t.tick(Inputs {
            ce: true,
            fence_ready: true,
            ..Inputs::default()
        });
        assert!(matches!(port.edge(o).unwrap(), Some(Command::Fence)));
        // Explicit event reference audit, plus bounded varying independent execution.
        let mut seed = 0x193a9d03u32;
        for edge in 0..4096 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let s = t.model.state;
            let command = if s.phase == Phase::Idle && seed & 4 != 0 {
                Some(Request {
                    kinds: 1 << ((seed >> 3) % 5),
                    token: ((seed >> 7) & 3) as u8,
                    validated: true,
                })
            } else {
                None
            };
            let i = Inputs {
                reset: edge % 128 == 0,
                ce: seed & 1 != 0,
                command,
                event_set: ((seed >> 16) & 255) as u8,
                event_ack: ((seed >> 24) & 255) as u8,
                dma_done: if seed & 256 != 0 { s.reserved } else { 0 },
                dma_ready: seed & 512 != 0,
                draw_ready: seed & 1024 != 0,
                draw_done: s.phase == Phase::DrawRun && seed & 2048 != 0,
                release_ready: seed & 4096 != 0,
                fence_ready: seed & 8192 != 0,
                external_fault: seed & 65535 == 0,
            };
            let old = t.model.state;
            let o = t.tick(i);
            if !i.reset {
                let mut ev = gpu_v2::command_processor::ports::EventState {
                    pending: old.pending,
                    cursor: old.cursor,
                };
                let mut set = i.event_set | (i.dma_done & old.reserved);
                if old.phase == Phase::Idle && i.command.is_some() {
                    set |= 16;
                }
                if o.fault {
                    set |= 128;
                }
                ev.update(set, 0);
                let picked =
                    ev.take(i.ce && matches!(old.phase, Phase::Idle | Phase::Wait | Phase::Fault));
                assert_eq!(picked, o.handler);
            }
        }
        assert_eq!(t.states, 255, "all actual coarse phases executed");
        assert!(t.handlers[5] == 0 && t.handlers[6] == 0);
        let receipt=format!("{{\"backend\":\"{backend}\",\"live_edges\":{},\"CE0\":{},\"captures\":{},\"command_done\":{},\"phase_mask\":{},\"handler_counts\":{:?},\"max_edges\":9000,\"reply_timeout_seconds\":5,\"mode\":\"live stdin actual RTL vs independently executed CommandEmu\"}}\n",t.edges,t.ce0,t.captures,t.done,t.states,t.handlers);
        fs::write(
            root.join("live").join(backend).join("receipt.json"),
            receipt,
        )
        .unwrap();
        println!(
            "{backend} PASS live independent command FSM {} edges, {} CE0, {} captures, {} done",
            t.edges, t.ce0, t.captures, t.done
        );
    }
}
