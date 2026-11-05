//! Read-only old-state facts. Only the root's successful clock publishes these.
//! IDs/ownership belong to the recorder, never to the model or its scheduler.
pub const CAPTURE_CONTRACT: &str = include_str!("profiling_contract.json");
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Event {
    pub kind: &'static str,
    pub values: Vec<u64>,
}

pub const LOAD_COLUMNS: [&str; 39] = [
    "dma",
    "vt_owned",
    "vt_step",
    "t_queue",
    "r_busy",
    "i_busy",
    "quad_total",
    "quad_producer",
    "light_pending",
    "sample_pending",
    "light_tokens",
    "sample_quads",
    "final_wait",
    "final_tokens",
    "rop_contexts",
    "r_quad",
    "i_pixel",
    "light_issue",
    "sample_issue",
    "final_issue",
    "rop_lane",
    "vt_owner",
    "vt_kind",
    "r_owner",
    "i_owner",
    "prepare_busy",
    "prepare_owner",
    "texture_work",
    "texture_reserved",
    "texture_public",
    "fb_hint",
    "fb_maintenance",
    "ro_wait",
    "fb_read_wait",
    "fb_write_wait",
    "ce",
    "light_tick",
    "sample_tick",
    "reset",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    pub events: Vec<Event>,
    pub load: [u64; 39],
    pub cache: Option<Cache>,
    pub control: Option<[u64; 9]>,
    pub control_word: Option<u32>,
}
impl Default for Trace {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            load: [0; 39],
            cache: None,
            control: None,
            control_word: None,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cache {
    pub valid: [bool; 8],
    pub tags: [u16; 8],
    pub color_base: u16,
    pub depth_base: u16,
}
impl Cache {
    /// Invalid tags carry no binding; valid=false disambiguates the zero code.
    pub fn valid_tags(&self) -> [u16; 8] {
        std::array::from_fn(|n| if self.valid[n] { self.tags[n] } else { 0 })
    }
}
impl Trace {
    pub(crate) fn event<const N: usize>(&mut self, kind: &'static str, values: [u64; N]) {
        self.events.push(Event {
            kind,
            values: values.into(),
        });
    }
}
