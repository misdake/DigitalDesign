//! Real producer/pool-to-join composition, with selected shader owners.
use super::{
    lighting::{self, Lighting},
    postquad::{self, Profile},
    producer::{self, Producer},
    sampling::pipeline::{self as sampler, Sampler},
    ContractError,
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub producer: producer::Input,
    /// Pool and reset fields are replaced by the actual producer wires.
    pub backend: postquad::Input,
    pub globals: lighting::Globals,
    pub sampling_config: sampler::Config,
    pub texture_memory: sampler::Memory,
    pub texture_invalidate: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub producer: producer::Output,
    pub backend: postquad::Output,
    pub idle: bool,
    pub light: Option<lighting::Output>,
    pub sampling: Option<sampler::Output>,
}
#[derive(Clone, Debug)]
pub struct Renderer {
    pub producer: Producer,
    pub backend: postquad::Join,
    pub lighting: Option<Lighting>,
    pub sampling: Option<Sampler>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Producer(producer::Error),
    Backend(ContractError),
    Lighting(String),
    Sampling(ContractError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Producer(e) => e.fmt(f),
            Self::Backend(e) => e.fmt(f),
            Self::Lighting(e) => e.fmt(f),
            Self::Sampling(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for Error {}
impl Renderer {
    pub fn with_triangles(profile: Profile) -> Self {
        Self {
            producer: Producer::with_triangles(),
            backend: postquad::Join {
                profile,
                ..Default::default()
            },
            lighting: None,
            sampling: None,
        }
    }
    pub fn with_lighting(profile: Profile) -> Self {
        Self {
            lighting: Some(Lighting::default()),
            ..Self::with_triangles(profile)
        }
    }
    pub fn with_sampling(profile: Profile) -> Self {
        Self {
            sampling: Some(Sampler::new(profile)),
            ..Self::with_lighting(profile)
        }
    }
    fn connect(
        &self,
        i: Input,
    ) -> (
        producer::Input,
        postquad::Input,
        Option<lighting::Input>,
        Option<sampler::Input>,
        Output,
    ) {
        let mut pi = i.producer;
        pi.backend.descriptor_issue_slot = self.backend.state.issue_slot;
        pi.backend.ce =
            i.backend.ce && !i.backend.abort && !self.backend.state.stopped && !pi.frontend.reset;
        pi.backend.publish_ready = true;
        let mut bi = postquad::Input {
            pool: self.producer.outputs(pi).pool,
            reset: pi.frontend.reset,
            ..i.backend
        };
        let li = self.lighting.as_ref().map(|_| lighting::Input {
            reset: bi.reset,
            ce: pi.backend.ce,
            compute_tick: pi.backend.ce,
            source_valid: bi.pool.light_valid,
            source_slot: bi.pool.light_descriptor as u8 & 31,
            source_lane: bi.pool.light_lane,
            source_mask: (bi.pool.light_descriptor >> 5) as u8 & 15,
            normal: bi.pool.light_normal,
            position: bi.pool.light_position,
            globals: i.globals,
            out_ready: pi.backend.ce,
        });
        let light = self.lighting.as_ref().zip(li).map(|(l, i)| l.outputs(i));
        if let Some(light) = light {
            bi.light = light.result;
            bi.light_source_ready = light.source_ready;
        }
        let si = self.sampling.as_ref().map(|_| sampler::Input {
            reset: bi.reset,
            ce: bi.ce,
            commit_allow: pi.backend.ce,
            abort: bi.abort,
            source_valid: bi.pool.sample_valid,
            source_descriptor: bi.pool.sample_descriptor,
            source_row: bi.pool.sample_row,
            source_data: bi.pool.sample_data,
            config: i.sampling_config,
            invalidate: i.texture_invalidate,
            packet_enable: true,
            result_ready: pi.backend.ce,
            memory: i.texture_memory,
        });
        let sampling = self.sampling.as_ref().zip(si).map(|(s, i)| s.outputs(i));
        if let Some(s) = sampling {
            bi.sample = s.result;
            bi.sample_source_ready = s.source_ready;
            bi.sample_publication = s.publication;
            bi.sampling_drained = s.drained;
        }
        pi.backend = self.backend.outputs(bi).feedback;
        let p = self.producer.outputs(pi);
        bi.pool = p.pool;
        let b = self.backend.outputs(bi);
        (
            pi,
            bi,
            li,
            si,
            Output {
                producer: p,
                backend: b,
                idle: p.frontend.idle && b.idle,
                light,
                sampling,
            },
        )
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.connect(i).4
    }
    pub fn clock(&mut self, i: Input) -> Result<(), Error> {
        let (pi, bi, li, si, _) = self.connect(i);
        let producer = self.producer.clock(pi);
        let backend = self.backend.clock(bi);
        let light = self
            .lighting
            .as_mut()
            .zip(li)
            .map_or(Ok(()), |(l, i)| l.clock(i));
        let sampling = self
            .sampling
            .as_mut()
            .zip(si)
            .map_or(Ok(()), |(s, i)| s.clock(i));
        producer.map_err(Error::Producer)?;
        backend.map_err(Error::Backend)?;
        light.map_err(Error::Lighting)?;
        sampling.map_err(Error::Sampling)
    }
}
