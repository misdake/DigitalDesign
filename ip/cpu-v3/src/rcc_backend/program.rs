//! Compiled image and structured backend failures.
use super::linker::ProgramValidationError;
use crate::Word;
use rcc::{AllocationError, InlineError};

#[derive(Clone, Debug)]
pub struct CpuV3Program {
    /// Profile of the source selected by the frontend, for software identity.
    pub gpu_debug: bool,
    /// Explicit entry CSEG; None retains the caller-selected legacy placement.
    pub cseg: Option<Word>,
    pub code_segments: Vec<CpuV3CodeSegment>,
    pub code_base: Word,
    pub words: Vec<Word>,
    pub listing: String,
    pub debug: rcc::DebugInfo,
}

/// An additional independently addressed 128-KiB instruction window.
#[derive(Clone, Debug)]
pub struct CpuV3CodeSegment {
    pub cseg: Word,
    pub code_base: Word,
    pub words: Vec<Word>,
    pub listing: String,
    pub debug: rcc::DebugInfo,
}
impl CpuV3Program {
    pub fn total_code_words(&self) -> usize {
        self.words.len()
            + self
                .code_segments
                .iter()
                .map(|s| s.words.len())
                .sum::<usize>()
    }
}

/// An error the CpuV3 backend reports after the frontend has accepted the
/// source program: a missing function, or a linked image that violates an
/// encoding requirement. Callers that want to display a compiler error should
/// use [`try_compile`](super::try_compile).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendError {
    /// A referenced function was not provided by the frontend.
    UnknownFunction(String),
    Cseg(String),
    /// The linked code image violated an encoding requirement.
    Validation(ProgramValidationError),
    Allocation(AllocationError),
    Inline(InlineError),
    Resource(BackendResourceError),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cseg(message) => f.write_str(message),
            Self::UnknownFunction(name) => write!(f, "unknown function `{name}`"),
            Self::Validation(error) => write!(f, "{error}"),
            Self::Allocation(error) => write!(f, "{error}"),
            Self::Inline(error) => write!(f, "{error}"),
            Self::Resource(error) => f.write_str(&error.message),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendResourceKind {
    CodeWindow,
    CodeDataOverlap,
    CodeStackOverlap,
    HeapRange,
    HeapStackOverlap,
    StaticCodeOverlap,
    StaticHeapOverlap,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendResourceError {
    pub kind: BackendResourceKind,
    pub message: String,
}
impl BackendError {
    pub(super) fn is_resource_limit(&self) -> bool {
        match self {
            Self::Resource(_) => true,
            Self::Allocation(e) => e.is_resource_limit(),
            _ => false,
        }
    }
}
pub(super) fn resource(kind: BackendResourceKind, message: String) -> BackendError {
    BackendError::Resource(BackendResourceError { kind, message })
}

impl std::error::Error for BackendError {}

impl From<ProgramValidationError> for BackendError {
    fn from(error: ProgramValidationError) -> Self {
        Self::Validation(error)
    }
}
