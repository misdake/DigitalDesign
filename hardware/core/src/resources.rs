//! Target resource declarations and allocation.
//!
//! Components declare the lower-level resources they reserve. Allocation is
//! transactional: either every requirement fits or none of them is applied.

use crate::{HardwareTarget, TargetResourceRequest};
use std::collections::{BTreeMap, HashSet};
use std::fmt::{Display, Formatter};
use std::marker::PhantomData;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ResourceKind {
    Lut4,
    FlipFlop,
    SsramBit,
    Bsram18K,
    Multiplier18x18,
    Pll,
    BoardClock27M,
    UserLed,
    UserButton,
    DebugUartTx,
    SdrSdramDevice,
    Ddr3Device,
    SpiFlashDevice,
    HdmiOutput,
}

impl Display for ResourceKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Lut4 => "LUT4",
            Self::FlipFlop => "flip-flop",
            Self::SsramBit => "SSRAM bit",
            Self::Bsram18K => "18 Kbit BSRAM block",
            Self::Multiplier18x18 => "18x18 multiplier",
            Self::Pll => "PLL",
            Self::BoardClock27M => "27 MHz board clock",
            Self::UserLed => "user LED",
            Self::UserButton => "user button",
            Self::DebugUartTx => "debug UART TX",
            Self::SdrSdramDevice => "SDR SDRAM device",
            Self::Ddr3Device => "DDR3 SDRAM device",
            Self::SpiFlashDevice => "SPI flash device",
            Self::HdmiOutput => "HDMI output",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceAmount {
    pub kind: ResourceKind,
    pub amount: u64,
}

impl ResourceAmount {
    pub const fn new(kind: ResourceKind, amount: u64) -> Self {
        Self { kind, amount }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TargetInventory {
    capacity: BTreeMap<ResourceKind, u64>,
    fitted_device_capacity_bits: BTreeMap<ResourceKind, u64>,
}

impl TargetInventory {
    pub fn new(capacity: impl IntoIterator<Item = ResourceAmount>) -> Self {
        let mut inventory = Self::default();
        inventory.extend(capacity);
        inventory
    }

    pub fn extend(&mut self, capacity: impl IntoIterator<Item = ResourceAmount>) {
        for resource in capacity {
            assert!(
                !matches!(
                    resource.kind,
                    ResourceKind::SdrSdramDevice
                        | ResourceKind::Ddr3Device
                        | ResourceKind::SpiFlashDevice
                ),
                "fitted memory devices must be added with `with_fitted_device`"
            );
            assert!(
                self.capacity
                    .insert(resource.kind, resource.amount)
                    .is_none(),
                "duplicate target resource capacity for {}",
                resource.kind
            );
        }
    }

    pub fn capacity(&self, kind: ResourceKind) -> u64 {
        self.capacity.get(&kind).copied().unwrap_or(0)
    }

    pub fn capacities(&self) -> &BTreeMap<ResourceKind, u64> {
        &self.capacity
    }

    /// Add one indivisible fitted memory device and retain its physical size
    /// as target metadata rather than allocatable capacity.
    pub fn with_fitted_device(mut self, kind: ResourceKind, capacity_bits: u64) -> Self {
        assert!(
            matches!(
                kind,
                ResourceKind::SdrSdramDevice
                    | ResourceKind::Ddr3Device
                    | ResourceKind::SpiFlashDevice
            ),
            "{kind} is not a fitted memory device"
        );
        assert!(capacity_bits > 0, "fitted device capacity must be non-zero");
        assert!(
            self.capacity.insert(kind, 1).is_none()
                && self
                    .fitted_device_capacity_bits
                    .insert(kind, capacity_bits)
                    .is_none(),
            "duplicate fitted device for {kind}"
        );
        self
    }

    pub fn fitted_device_capacity_bits(&self, kind: ResourceKind) -> Option<u64> {
        self.fitted_device_capacity_bits.get(&kind).copied()
    }

    pub fn fitted_devices(&self) -> &BTreeMap<ResourceKind, u64> {
        &self.fitted_device_capacity_bits
    }
}

/// A configurable component that can be reserved from a hardware target.
///
/// The returned list deliberately supports multiple lower-level resources.
/// Future BSRAM/DSP allocators can replace a simple fixed count with packing,
/// port-mode and alternative-implementation planning without changing
/// `TargetResources::take`.
pub trait TargetComponent: Sized + 'static {
    fn component_name(&self) -> &'static str;
    fn resource_requirements(&self) -> Vec<ResourceAmount>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceAllocation {
    pub id: u64,
    pub label: String,
    pub component: &'static str,
    pub resources: Vec<ResourceAmount>,
}

#[derive(Clone, Debug)]
pub struct ResourceLease<T: HardwareTarget, C> {
    pub id: u64,
    pub component: C,
    target: PhantomData<T>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceReport {
    pub target: &'static str,
    pub capacity: BTreeMap<ResourceKind, u64>,
    pub claimed: BTreeMap<ResourceKind, u64>,
    pub fitted_device_capacity_bits: BTreeMap<ResourceKind, u64>,
    pub allocations: Vec<ResourceAllocation>,
}

impl ResourceReport {
    pub fn remaining(&self, kind: ResourceKind) -> u64 {
        self.capacity
            .get(&kind)
            .copied()
            .unwrap_or(0)
            .saturating_sub(self.claimed.get(&kind).copied().unwrap_or(0))
    }
}

pub struct TargetResources<T: HardwareTarget> {
    inventory: TargetInventory,
    claimed: BTreeMap<ResourceKind, u64>,
    allocations: Vec<ResourceAllocation>,
    labels: HashSet<String>,
    next_id: u64,
    failed: Option<String>,
    target: PhantomData<T>,
}

impl<T: HardwareTarget> TargetResources<T> {
    pub fn new() -> Self {
        Self {
            inventory: T::inventory(),
            claimed: BTreeMap::new(),
            allocations: Vec::new(),
            labels: HashSet::new(),
            next_id: 0,
            failed: None,
            target: PhantomData,
        }
    }

    pub fn take<C>(&mut self, component: C) -> Result<ResourceLease<T, C>, ResourceError>
    where
        C: TargetComponent,
    {
        let label = format!("{}-{}", component.component_name(), self.next_id);
        self.take_named(label, component)
    }

    pub fn take_named<C>(
        &mut self,
        label: impl Into<String>,
        component: C,
    ) -> Result<ResourceLease<T, C>, ResourceError>
    where
        C: TargetComponent,
    {
        if let Some(reason) = &self.failed {
            return Err(ResourceError::AllocatorFailed {
                target: T::NAME,
                reason: reason.clone(),
            });
        }
        let label = label.into();
        let result = self.try_take_named(label, component);
        if let Err(error) = &result {
            self.failed = Some(error.to_string());
        }
        result
    }

    fn try_take_named<C>(
        &mut self,
        label: String,
        component: C,
    ) -> Result<ResourceLease<T, C>, ResourceError>
    where
        C: TargetComponent,
    {
        let id = self.try_claim(
            label,
            component.component_name(),
            component.resource_requirements(),
        )?;
        Ok(ResourceLease {
            id,
            component,
            target: PhantomData,
        })
    }

    pub fn claim_module(
        &mut self,
        label: String,
        request: &TargetResourceRequest,
    ) -> Result<(), ResourceError> {
        if let Some(reason) = &self.failed {
            return Err(ResourceError::AllocatorFailed {
                target: T::NAME,
                reason: reason.clone(),
            });
        }
        let result = self
            .try_claim(label, request.component, request.resources.clone())
            .map(|_| ());
        if let Err(error) = &result {
            self.failed = Some(error.to_string());
        }
        result
    }

    fn try_claim(
        &mut self,
        label: String,
        component: &'static str,
        requirements: Vec<ResourceAmount>,
    ) -> Result<u64, ResourceError> {
        if self.labels.contains(&label) {
            return Err(ResourceError::DuplicateLabel(label));
        }

        let requirements = normalize_requirements(requirements)?;
        for requirement in &requirements {
            let capacity = self.inventory.capacity(requirement.kind);
            if capacity == 0 {
                return Err(ResourceError::Unavailable {
                    target: T::NAME,
                    component,
                    resource: requirement.kind,
                });
            }
            let already_claimed = self.claimed.get(&requirement.kind).copied().unwrap_or(0);
            let requested = already_claimed.checked_add(requirement.amount).ok_or(
                ResourceError::ArithmeticOverflow {
                    component,
                    resource: requirement.kind,
                },
            )?;
            if requested > capacity {
                return Err(ResourceError::CapacityExceeded {
                    target: T::NAME,
                    component,
                    resource: requirement.kind,
                    requested: requirement.amount,
                    remaining: capacity - already_claimed,
                });
            }
        }

        for requirement in &requirements {
            *self.claimed.entry(requirement.kind).or_default() += requirement.amount;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.labels.insert(label.clone());
        self.allocations.push(ResourceAllocation {
            id,
            label,
            component,
            resources: requirements,
        });
        Ok(id)
    }

    /// Reject export after any failed allocation, even if its `Result` was
    /// caught or explicitly ignored by the caller.
    pub fn ensure_valid(&self) -> Result<(), ResourceError> {
        match &self.failed {
            Some(reason) => Err(ResourceError::AllocatorFailed {
                target: T::NAME,
                reason: reason.clone(),
            }),
            None => Ok(()),
        }
    }

    pub fn report(&self) -> ResourceReport {
        ResourceReport {
            target: T::NAME,
            capacity: self.inventory.capacities().clone(),
            claimed: self.claimed.clone(),
            fitted_device_capacity_bits: self.inventory.fitted_devices().clone(),
            allocations: self.allocations.clone(),
        }
    }
}

impl<T: HardwareTarget> Default for TargetResources<T> {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_requirements(
    requirements: Vec<ResourceAmount>,
) -> Result<Vec<ResourceAmount>, ResourceError> {
    let mut normalized = BTreeMap::<ResourceKind, u64>::new();
    for requirement in requirements {
        if requirement.amount == 0 {
            continue;
        }
        let amount = normalized.entry(requirement.kind).or_default();
        *amount =
            amount
                .checked_add(requirement.amount)
                .ok_or(ResourceError::RequirementOverflow {
                    resource: requirement.kind,
                })?;
    }
    Ok(normalized
        .into_iter()
        .map(|(kind, amount)| ResourceAmount { kind, amount })
        .collect())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceError {
    AllocatorFailed {
        target: &'static str,
        reason: String,
    },
    DuplicateLabel(String),
    Unavailable {
        target: &'static str,
        component: &'static str,
        resource: ResourceKind,
    },
    CapacityExceeded {
        target: &'static str,
        component: &'static str,
        resource: ResourceKind,
        requested: u64,
        remaining: u64,
    },
    ArithmeticOverflow {
        component: &'static str,
        resource: ResourceKind,
    },
    RequirementOverflow {
        resource: ResourceKind,
    },
}

impl Display for ResourceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllocatorFailed { target, reason } => write!(
                formatter,
                "resource allocation for target `{target}` already failed: {reason}"
            ),
            Self::DuplicateLabel(label) => write!(formatter, "duplicate resource label `{label}`"),
            Self::Unavailable {
                target,
                component,
                resource,
            } => write!(
                formatter,
                "component `{component}` requires unavailable resource {resource} on target `{target}`"
            ),
            Self::CapacityExceeded {
                target,
                component,
                resource,
                requested,
                remaining,
            } => write!(
                formatter,
                "component `{component}` requests {requested} {resource}, but target `{target}` has {remaining} remaining"
            ),
            Self::ArithmeticOverflow {
                component,
                resource,
            } => write!(
                formatter,
                "resource accounting overflow for component `{component}` and {resource}"
            ),
            Self::RequirementOverflow { resource } => {
                write!(formatter, "component requirement overflow for {resource}")
            }
        }
    }
}

impl std::error::Error for ResourceError {}

pub mod components {
    use super::{ResourceAmount, ResourceKind, TargetComponent};

    macro_rules! fixed_component {
        ($name:ident, $display:literal, $kind:ident) => {
            #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
            pub struct $name;

            impl TargetComponent for $name {
                fn component_name(&self) -> &'static str {
                    $display
                }

                fn resource_requirements(&self) -> Vec<ResourceAmount> {
                    vec![ResourceAmount::new(ResourceKind::$kind, 1)]
                }
            }
        };
    }

    fixed_component!(Pll, "pll", Pll);
    fixed_component!(Clock27M, "clock-27mhz", BoardClock27M);
    fixed_component!(DebugUartTx, "debug-uart-tx", DebugUartTx);
    fixed_component!(HdmiOutput, "hdmi-output", HdmiOutput);
    fixed_component!(SdrSdram, "sdr-sdram", SdrSdramDevice);
    fixed_component!(Ddr3, "ddr3-sdram", Ddr3Device);
    fixed_component!(SpiFlash, "spi-flash", SpiFlashDevice);

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct BsramBlocks {
        pub blocks: u64,
    }

    impl BsramBlocks {
        pub const fn new(blocks: u64) -> Self {
            Self { blocks }
        }
    }

    impl TargetComponent for BsramBlocks {
        fn component_name(&self) -> &'static str {
            "bsram"
        }

        fn resource_requirements(&self) -> Vec<ResourceAmount> {
            vec![ResourceAmount::new(ResourceKind::Bsram18K, self.blocks)]
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct SsramBits {
        pub bits: u64,
    }

    impl SsramBits {
        pub const fn new(bits: u64) -> Self {
            Self { bits }
        }
    }

    impl TargetComponent for SsramBits {
        fn component_name(&self) -> &'static str {
            "ssram"
        }

        fn resource_requirements(&self) -> Vec<ResourceAmount> {
            vec![ResourceAmount::new(ResourceKind::SsramBit, self.bits)]
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct DspMultipliers {
        pub multipliers: u64,
    }

    impl DspMultipliers {
        pub const fn new(multipliers: u64) -> Self {
            Self { multipliers }
        }
    }

    impl TargetComponent for DspMultipliers {
        fn component_name(&self) -> &'static str {
            "dsp-multipliers"
        }

        fn resource_requirements(&self) -> Vec<ResourceAmount> {
            vec![ResourceAmount::new(
                ResourceKind::Multiplier18x18,
                self.multipliers,
            )]
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct UserLeds<const COUNT: u32>;

    impl<const COUNT: u32> TargetComponent for UserLeds<COUNT> {
        fn component_name(&self) -> &'static str {
            "user-leds"
        }

        fn resource_requirements(&self) -> Vec<ResourceAmount> {
            vec![ResourceAmount::new(ResourceKind::UserLed, u64::from(COUNT))]
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct UserButtons<const COUNT: u32>;

    impl<const COUNT: u32> TargetComponent for UserButtons<COUNT> {
        fn component_name(&self) -> &'static str {
            "user-buttons"
        }

        fn resource_requirements(&self) -> Vec<ResourceAmount> {
            vec![ResourceAmount::new(
                ResourceKind::UserButton,
                u64::from(COUNT),
            )]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::components::{BsramBlocks, DspMultipliers, SdrSdram};
    use super::*;
    use crate::HardwareBackend;

    #[derive(Debug)]
    struct TestTarget;
    impl HardwareTarget for TestTarget {
        type Backend = TestBackend;
        const NAME: &'static str = "test";
        fn inventory() -> TargetInventory {
            TargetInventory::new([
                ResourceAmount::new(ResourceKind::Bsram18K, 46),
                ResourceAmount::new(ResourceKind::Multiplier18x18, 32),
                ResourceAmount::new(ResourceKind::Pll, 2),
            ])
            .with_fitted_device(ResourceKind::SdrSdramDevice, 64 * 1_024 * 1_024)
        }
    }
    struct TestBackend;
    impl HardwareBackend for TestBackend {
        const NAME: &'static str = "test";
    }

    #[test]
    fn configurable_resources_use_component_declared_quantities() {
        let mut resources = TargetResources::<TestTarget>::new();
        resources
            .take_named("frame-buffers", BsramBlocks::new(12))
            .unwrap();
        resources
            .take_named("geometry", DspMultipliers::new(5))
            .unwrap();

        let report = resources.report();
        assert_eq!(report.claimed[&ResourceKind::Bsram18K], 12);
        assert_eq!(report.claimed[&ResourceKind::Multiplier18x18], 5);
        assert_eq!(report.remaining(ResourceKind::Bsram18K), 34);
    }

    #[test]
    fn fitted_memory_is_an_indivisible_device() {
        let mut resources = TargetResources::<TestTarget>::new();
        resources.take_named("memory", SdrSdram).unwrap();
        let error = resources.take_named("second-memory", SdrSdram).unwrap_err();

        let report = resources.report();
        assert_eq!(report.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(report.remaining(ResourceKind::SdrSdramDevice), 0);
        assert_eq!(
            report.fitted_device_capacity_bits[&ResourceKind::SdrSdramDevice],
            64 * 1_024 * 1_024
        );
        assert!(matches!(
            error,
            ResourceError::CapacityExceeded {
                resource: ResourceKind::SdrSdramDevice,
                remaining: 0,
                ..
            }
        ));
    }

    #[test]
    fn failed_take_does_not_partially_claim_resources() {
        #[derive(Clone, Copy, Debug)]
        struct OversizedVideo;

        impl TargetComponent for OversizedVideo {
            fn component_name(&self) -> &'static str {
                "oversized-video"
            }

            fn resource_requirements(&self) -> Vec<ResourceAmount> {
                vec![
                    ResourceAmount::new(ResourceKind::Multiplier18x18, 3),
                    ResourceAmount::new(ResourceKind::Pll, 3),
                ]
            }
        }

        let mut resources = TargetResources::<TestTarget>::new();
        let error = resources.take(OversizedVideo).unwrap_err();
        assert!(matches!(
            error,
            ResourceError::CapacityExceeded {
                resource: ResourceKind::Pll,
                ..
            }
        ));
        assert_eq!(
            resources
                .report()
                .claimed
                .get(&ResourceKind::Multiplier18x18),
            None
        );
    }
}
