//! Reviewed module calendars with one full arithmetic path for all lit modes.
//! Plan fixtures are generated from the offline experiment, never rescheduled
//! implicitly. Legacy constructors retain their independently qualified paths.
use super::{
    rtl::{DspSteering, LightingRtlOptions},
    sim::workbench::{SchedulePlan, Slot},
    LightingProfile, LightingQuantization,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnifiedCalendar {
    /// Selected smooth H9 calendar: global seven-lane pool and one fused MAC.
    SmoothH9,
    /// Frozen 34-edge H9 reference with the original nine lanes and two MACs.
    SmoothH9Reference,
    /// Original per-edge schedule with optimized DSP input connections.
    Free,
    /// DSP ownership restricted to at most two neighboring two-edge groups.
    TwoEdge,
}

impl UnifiedCalendar {
    pub fn selected(quantization: LightingQuantization) -> Self {
        match quantization {
            LightingQuantization::CompensatedFloor => Self::Free,
            LightingQuantization::NearestEven => Self::TwoEdge,
        }
    }

    pub fn options(self, quantization: LightingQuantization) -> LightingRtlOptions {
        LightingRtlOptions {
            smooth_h9: matches!(self, Self::SmoothH9 | Self::SmoothH9Reference),
            h9_tail_dsp: matches!(self, Self::SmoothH9 | Self::SmoothH9Reference),
            dsp_steering: if self == Self::SmoothH9 {
                DspSteering::Orient
            } else {
                self.options_legacy(quantization).dsp_steering
            },
            power_context_masks: self == Self::Free
                && quantization == LightingQuantization::CompensatedFloor,
            rsqrt_q13: self != Self::TwoEdge
                && quantization == LightingQuantization::CompensatedFloor,
            ..self.options_legacy(quantization)
        }
    }
    /// Retained exact Q15 endpoint baseline for numerical/resource comparisons.
    pub fn options_legacy(self, quantization: LightingQuantization) -> LightingRtlOptions {
        LightingRtlOptions {
            unified_lit: true,
            dsp_steering: if self != Self::TwoEdge {
                DspSteering::Local
            } else {
                DspSteering::Orient
            },
            id_ring: self == Self::TwoEdge
                && quantization == LightingQuantization::CompensatedFloor,
            ..LightingRtlOptions::lit_queue_resource_profile(LightingProfile::Fast, quantization)
        }
    }

    pub fn plans(self, quantization: LightingQuantization) -> Result<[SchedulePlan; 2], String> {
        if matches!(self, Self::SmoothH9 | Self::SmoothH9Reference)
            && quantization != LightingQuantization::CompensatedFloor
        {
            return Err("smooth h9 requires the reviewed compensated-floor geometry".into());
        }
        let text = if self == Self::SmoothH9 {
            include_str!("../../spec/lighting-calendars/shared-global7-h9.plan")
        } else if self == Self::SmoothH9Reference {
            include_str!("../../spec/lighting-calendars/free-h9.plan")
        } else if self.options(quantization).rsqrt_q13 {
            include_str!("../../spec/lighting-calendars/free-floor-q13.plan")
        } else {
            self.plan_text(quantization)
        };
        Self::parse_plan(text)
    }
    pub fn plans_legacy(
        self,
        quantization: LightingQuantization,
    ) -> Result<[SchedulePlan; 2], String> {
        Self::parse_plan(
            if matches!(self, Self::SmoothH9 | Self::SmoothH9Reference) {
                Self::Free.plan_text(quantization)
            } else {
                self.plan_text(quantization)
            },
        )
    }
    fn plan_text(self, quantization: LightingQuantization) -> &'static str {
        match (self, quantization) {
            (Self::SmoothH9 | Self::SmoothH9Reference, _) => Self::Free.plan_text(quantization),
            (Self::Free, LightingQuantization::CompensatedFloor) => {
                include_str!("../../spec/lighting-calendars/free-floor.plan")
            }
            (Self::Free, LightingQuantization::NearestEven) => {
                include_str!("../../spec/lighting-calendars/free-rne.plan")
            }
            (Self::TwoEdge, LightingQuantization::CompensatedFloor) => {
                include_str!("../../spec/lighting-calendars/two-edge-floor.plan")
            }
            (Self::TwoEdge, LightingQuantization::NearestEven) => {
                include_str!("../../spec/lighting-calendars/two-edge-rne.plan")
            }
        }
    }
    fn parse_plan(text: &str) -> Result<[SchedulePlan; 2], String> {
        let mut lines = text.lines();
        let ii = lines
            .next()
            .ok_or("missing II")?
            .parse()
            .map_err(|_| "invalid II")?;
        let capacities = lines
            .next()
            .ok_or("missing capacities")?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| "invalid capacities")?;
        if lines.next() != Some("id,issue,lane") {
            return Err("invalid calendar columns".into());
        }
        let slots = lines
            .map(|line| {
                let fields = line.split(',').collect::<Vec<_>>();
                if fields.len() != 3 {
                    return Err("invalid slot columns");
                }
                Ok(Slot {
                    id: fields[0].parse().map_err(|_| "invalid node")?,
                    issue: fields[1].parse().map_err(|_| "invalid issue")?,
                    lane: fields[2].parse().map_err(|_| "invalid lane")?,
                })
            })
            .collect::<Result<_, _>>()?;
        let plan = SchedulePlan {
            ii,
            capacities,
            slots,
            preconnected_modes: true,
        };
        Ok([plan.clone(), plan])
    }
}
