//! Shared peer contracts are registered once in the owning host suite.
use super::{shared_support_record_raster as record, shared_support_sdram_shared_pixel as memory};

#[test]
fn one_parent_and_one_edge_sink_reject_double_admission_or_tick() {
    memory::qualification::one_parent_and_one_edge_sink_reject_double_admission_or_tick();
}

#[test]
fn source_underrun_after_ro_presentation_poison_prevents_retry() {
    memory::qualification::source_underrun_after_ro_presentation_poison_prevents_retry();
}

#[test]
fn reference_tick_failure_poison_prevents_retry() {
    memory::qualification::reference_tick_failure_poison_prevents_retry();
}

#[test]
fn ndc_signed_ties() {
    record::ndc_signed_ties();
}
