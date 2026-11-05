//! Installed CP/DMA/source RAM connected to the actual producer and TDP path.
//! Selected Lighting and Sampling consume real pool tuples. The caller owns
//! physical feedback and its unique clock.
use super::{
    command::{self, Control, DeviceBus},
    frontend,
    postquad::{self, Profile},
    prepare::Params,
    producer,
    render::{self, Renderer},
    router, ui, ContractError, Physical, Request,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub abort: bool,
    pub bus: DeviceBus,
    pub physical: [Physical; 3],
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub control: command::Output,
    pub render: render::Output,
    pub physical_drained: bool,
    pub ui: Option<ui::Output>,
    /// Root memory pins, including the CLEAR bypass of the router.
    pub physical: [Request; 3],
    pub write_data: u64,
}
#[derive(Clone, Debug, Default)]
pub struct Edge {
    pub command: command::CommandEdge,
    pub trace: super::trace::Trace,
}
#[derive(Clone, Debug)]
pub struct Gpu {
    pub control: Control,
    pub render: Renderer,
    pub ui: Option<ui::Engine>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Control(command::Error),
    Render(render::Error),
    Auxiliary(ContractError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Control(e) => e.fmt(f),
            Self::Render(e) => e.fmt(f),
            Self::Auxiliary(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for Error {}
impl Gpu {
    /// Complete command profile; physical memory has one external clock owner.
    pub fn with_all_commands(profile: Profile) -> Self {
        Self {
            control: Control::with_all_commands(),
            render: Renderer::with_sampling(profile),
            ui: Some(ui::Engine::default()),
        }
    }
    pub fn with_draws(profile: Profile) -> Self {
        Self {
            control: Control::with_draws(),
            render: Renderer::with_sampling(profile),
            ui: None,
        }
    }
    fn connect(&self, i: Input) -> (command::Input, render::Input, Option<ui::Input>, Output) {
        let mut ci = command::Input {
            reset: i.reset,
            ce: i.ce,
            abort: i.abort,
            bus: i.bus,
            physical: i.physical,
            ..Default::default()
        };
        let mut previous = None;
        for _ in 0..8 {
            let co = self.control.outputs(ci);
            let p = co.processor;
            let d = co.transport.dma;
            let t = co.transport.router;
            let c = p.bindings;
            let ui_input = self.ui.as_ref().map(|_| ui::Input {
                reset: i.reset,
                ce: i.ce,
                start: p.auxiliary.ui_start,
                stop: p.stop,
                grid_base: ci.ram_q as u16,
                atlas_base: (ci.ram_q >> 16) as u8 & 127,
                first_cell: p.auxiliary.ui_first,
                last_cell: p.auxiliary.ui_last,
                cp_ready: ci.ram_ready,
                cp_q: ci.ram_q,
                work_ready: d.work_ready,
                dma_idle: d.idle,
                command_ready: ci.auxiliary.copy_ready,
                command_done: ci.auxiliary.copy_complete,
            });
            let u = self
                .ui
                .as_ref()
                .zip(ui_input)
                .map(|(engine, input)| engine.outputs(input));
            let ui = u.unwrap_or_default();
            let a = p.auxiliary;
            let front_ce = i.ce && !p.stop && !self.render.backend.state.stopped;
            let ri = render::Input {
                sampling_config: super::sampling::pipeline::Config {
                    base: c.texture_base,
                    n: c.texture_n,
                    mip: c.texture_mip,
                    filter: c.texture_filter,
                    framebuffer: c.texture_framebuffer,
                },
                texture_invalidate: p.texture_invalidate,
                texture_memory: super::sampling::pipeline::Memory {
                    request_ready: t.texture_ready,
                    read: t.texture_read,
                    index: t.texture_index,
                    data: t.texture_data,
                    complete: t.texture_complete,
                    ok: t.texture_ok,
                },
                globals: super::lighting::Globals {
                    epoch: c.epoch,
                    mode: c.mode,
                    code: c.code,
                    light: c.light,
                    ambient: c.ambient,
                    directional: c.directional,
                    ray: c.ray,
                },
                producer: producer::Input {
                    frontend: frontend::Input {
                        reset: i.reset,
                        ce: front_ce,
                        start: p.start,
                        slot: p.draw_slot,
                        mesh_color: c.mesh_color,
                        dma_write: d.ram_write,
                        dma_address: d.ram_address,
                        dma_data: d.ram_data,
                        output_ready: true,
                        cp_read: if ui.active { ui.cp_read } else { p.ram_read },
                        cp_address: if ui.active {
                            ui.cp_address
                        } else {
                            p.ram_address
                        },
                        params: Params {
                            far_z: c.far_z,
                            tex_u: c.texture_n,
                            tex_v: c.texture_n,
                            max_mip: if c.texture_mip { c.texture_n } else { 0 },
                            bias: c.lod_bias,
                        },
                        ..Default::default()
                    },
                    draw_unlit: c.unlit,
                    ..Default::default()
                },
                backend: postquad::Input {
                    serial: super::tdp::Serial {
                        active: ui.active || a.copy_active,
                        command_valid: if a.copy_active {
                            a.copy_request
                        } else {
                            ui.command_valid
                        },
                        writing: if a.copy_active {
                            a.copy_write
                        } else {
                            ui.writing
                        },
                        lower_ready: a.copy_active || ui.lower_ready,
                        tile: ui.tile,
                        copy: a.copy_active,
                        copy_address: a.copy_address,
                        pixel_valid: ui.pixel_valid,
                        pixel_half: ui.pixel_half,
                        pixel_word: ui.pixel_word,
                        pixels: super::vertex::Bits::new(ui.pixels, 64),
                    },
                    reset: i.reset,
                    ce: i.ce,
                    abort: p.stop,
                    specular: c.specular,
                    depth_func: c.depth_func,
                    depth_write: c.depth_write,
                    blend: c.blend,
                    color_base: c.color_base,
                    depth_base: c.depth_base,
                    flush_request: p.flush_request,
                    memory: postquad::Memory {
                        request_ready: t.framebuffer_ready && !a.clear_active,
                        read_data: t.framebuffer_data,
                        read_valid: t.framebuffer_read && !a.clear_active,
                        write_ready: t.framebuffer_write_ready && !a.clear_active,
                        complete: t.framebuffer_complete && !a.clear_active,
                        ok: t.framebuffer_ok,
                    },
                    ..Default::default()
                },
            };
            let ro = self.render.outputs(ri);
            let f = ro.producer.frontend;
            let cache = ro.backend.sink.cache;
            ci.ram_ready = f.meshlet.cp_ready;
            ci.ram_q = f.meshlet.cp_q.value;
            ci.draw.meshlet_ready = f.meshlet.ready;
            ci.draw.meshlet_done = f.meshlet.done;
            ci.render_idle = ro.idle;
            ci.render_fault = ro.backend.fault || ro.sampling.is_some_and(|s| s.fault);
            ci.flush_complete = cache.flush_complete;
            ci.clear_active = a.clear_active;
            ci.auxiliary = command::AuxiliaryInput {
                clear_ready: i.physical[2].request_ready,
                clear_complete: i.physical[2].response.valid
                    && (i.physical[2].response.last || i.physical[2].response.error),
                clear_ok: !i.physical[2].response.error,
                ui_done: ui.done,
                copy_ready: cache.serial_ready,
                copy_complete: cache.serial_done,
            };
            ci.auxiliary_work = ui.active.then_some(super::command_dma::Work {
                valid: ui.work_valid,
                source_halfword: ui.work_source,
                destination: ui.work_destination,
                size: 2,
                ..Default::default()
            });
            ci.framebuffer = router::Framebuffer {
                valid: cache.mem_req_valid,
                writing: cache.mem_req_write,
                address_bytes: cache.mem_req_address,
                write_valid: cache.mem_write_valid,
                write_data: cache.mem_write_data.value,
            };
            if let Some(s) = ro.sampling {
                ci.texture = router::Texture {
                    valid: s.cache.mem_req_valid,
                    address_bytes: s.cache.mem_req_addr,
                };
            }
            let key = (
                ci.ram_ready,
                ci.ram_q,
                (ci.render_idle, ci.render_fault, ci.flush_complete),
                (ci.draw.meshlet_ready, ci.draw.meshlet_done),
                ci.framebuffer,
                ci.texture,
                ci.clear_active,
                ci.auxiliary_work,
                (
                    ci.auxiliary.clear_ready,
                    ci.auxiliary.clear_complete,
                    ci.auxiliary.clear_ok,
                    ci.auxiliary.ui_done,
                    ci.auxiliary.copy_ready,
                    ci.auxiliary.copy_complete,
                ),
            );
            if previous == Some(key) {
                return (
                    ci,
                    ri,
                    ui_input,
                    Output {
                        control: co,
                        render: ro,
                        physical_drained: co.transport.dma.idle
                            && !co.transport.router.busy
                            && !self.render.backend.sink.cache.state.mt_active
                            && !self.render.backend.sink.cache.state.mt_request
                            && !a.clear_active
                            && !a.copy_active
                            && !ui.active
                            && ro.sampling.is_none_or(|s| s.drained),
                        ui: u,
                        physical: {
                            let mut ports = t.physical;
                            if a.clear_active {
                                ports[2] = Request {
                                    valid: a.clear_request,
                                    address_halfword: a.clear_address >> 1,
                                    count: 3,
                                };
                            }
                            ports
                        },
                        write_data: if a.clear_active {
                            u64::from(a.clear_value) * 0x0001_0001_0001_0001
                        } else {
                            t.write_data
                        },
                    },
                );
            }
            previous = Some(key);
        }
        panic!("GPU CP/render old-state wiring exceeded8 settle passes");
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.connect(i).3
    }
    pub fn clock(&mut self, i: Input) -> Result<(), Error> {
        self.clock_inner(i, false).map(|_| ())
    }
    pub fn clock_observing(&mut self, i: Input) -> Result<Edge, Error> {
        self.clock_inner(i, true)
    }
    fn clock_inner(&mut self, i: Input, observing: bool) -> Result<Edge, Error> {
        let (ci, ri, ui, output) = self.connect(i);
        let target_was_valid = self.control.processor.draw_status().is_some_and(|s| s.2);
        let mut trace = super::trace::Trace::default();
        if observing {
            self.control.observe(ci, &mut trace);
            self.render.observe(ri, &mut trace);
            trace.load[35] = u64::from(i.ce);
            trace.load[38] = u64::from(i.reset);
            for (n, port) in output.physical.into_iter().enumerate() {
                trace.load[32 + n] = u64::from(port.valid && !i.physical[n].request_ready);
                if port.valid && i.physical[n].request_ready {
                    trace.event(
                        "master_request",
                        [
                            n as u64,
                            u64::from(port.address_halfword),
                            u64::from(port.count),
                        ],
                    );
                }
                let response = i.physical[n].response;
                if !i.reset && (n < 2 && response.valid || n == 2 && i.physical[n].write_ready) {
                    let data = if n == 2 {
                        output.write_data
                    } else {
                        response.data
                    };
                    trace.event(
                        "master_payload",
                        [
                            n as u64,
                            data as u32 as u64,
                            data >> 32,
                            255,
                            u64::from(n < 2 && response.error),
                        ],
                    );
                }
                if !i.reset && response.valid && (response.last || response.error) {
                    trace.event("master_terminal", [n as u64, u64::from(response.error)]);
                }
            }
            if output.physical[2].valid && i.physical[2].request_ready {
                let a = output.control.processor.auxiliary;
                let mode = if a.clear_active {
                    1
                } else if a.copy_active {
                    3
                } else if output.ui.is_some_and(|u| u.active) {
                    2
                } else {
                    0
                };
                trace.event("write_owner", [mode]);
            }
        }
        if !i.reset && ci.clear_active {
            super::require(
                output.render.idle && !ci.framebuffer.valid,
                "gpu",
                "CLEAR overlapped compute/cache work",
            )
            .map_err(Error::Auxiliary)?;
        }
        let control = self.control.clock_observing(ci);
        let render = self.render.clock(ri);
        let auxiliary = self
            .ui
            .as_mut()
            .zip(ui)
            .map(|(engine, input)| engine.clock(input))
            .transpose();
        let edge = control.map_err(Error::Control)?;
        render.map_err(Error::Render)?;
        auxiliary.map_err(Error::Auxiliary)?;
        if observing {
            if target_was_valid && !self.control.processor.draw_status().is_some_and(|s| s.2) {
                trace.event("target_unbind", [u64::from(i.reset)]);
            }
            if let Some((color, depth, valid)) = edge.target {
                trace.event(
                    "target",
                    [
                        u64::from(color) * 512,
                        u64::from(depth) * 512,
                        u64::from(valid),
                    ],
                );
            }
        }
        Ok(Edge {
            command: edge,
            trace,
        })
    }
}
