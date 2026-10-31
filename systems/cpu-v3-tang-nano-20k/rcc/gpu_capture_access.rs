//! Legacy root selection for the shared read-only UART consumer.
//! The explicit-API loader supplies the same re-exports from gpu_api instead.
mod gpu_pipeline;
pub use gpu_pipeline::{gpu_capture_error, gpu_capture_range_word, gpu_capture_read_word};
