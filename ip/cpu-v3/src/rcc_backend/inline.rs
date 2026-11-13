//! Independent baseline/candidate lowering and bounded target-cost selection.
use super::*;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InlineRejection {
    Resource(String),
    CodeGrowth {
        baseline: usize,
        candidate: usize,
        allowance: usize,
    },
    FrameGrowth {
        function: FuncName,
        baseline: usize,
        candidate: usize,
    },
    FpuSpillGrowth {
        function: FuncName,
        baseline: usize,
        candidate: usize,
    },
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum InlineOutcome {
    Disabled,
    #[default]
    NoExpansions,
    Applied,
    Baseline(InlineRejection),
}
#[derive(Clone, Debug)]
pub struct InlineFrameCost {
    pub function: FuncName,
    pub baseline_frame: usize,
    pub candidate_frame: usize,
    pub baseline_gpr_spill: usize,
    pub candidate_gpr_spill: usize,
    pub baseline_fpu_spill: usize,
    pub candidate_fpu_spill: usize,
}
impl InlineFrameCost {
    /// A caller-local soft rejection; thresholds do not disable other callers.
    pub fn rejection(&self) -> Option<InlineRejection> {
        if self.candidate_frame > self.baseline_frame + 16 {
            Some(InlineRejection::FrameGrowth {
                function: self.function,
                baseline: self.baseline_frame,
                candidate: self.candidate_frame,
            })
        } else if self.candidate_fpu_spill > self.baseline_fpu_spill + 8 {
            Some(InlineRejection::FpuSpillGrowth {
                function: self.function,
                baseline: self.baseline_fpu_spill,
                candidate: self.candidate_fpu_spill,
            })
        } else {
            None
        }
    }
}
#[derive(Clone, Debug)]
pub struct InlineSourceOrigin {
    pub site: usize,
    pub caller_file: Option<String>,
    pub source_file: Option<String>,
    pub callee_file: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct InlineCompilationReport {
    pub ir: InlineReport,
    pub origins: Vec<InlineSourceOrigin>,
    pub outcome: InlineOutcome,
    pub retained_sites: usize,
    pub fallback_count: usize,
    pub baseline_words: Option<usize>,
    pub candidate_words: Option<usize>,
    pub frames: Vec<InlineFrameCost>,
    pub baseline_lowering: Duration,
    pub candidate_lowering: Duration,
}
impl std::fmt::Display for InlineCompilationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f,"inline: {:?}; {} attempted expansions ({} automatic), {} retained, {} cumulative nodes, {} fallbacks; baseline {:?} words, candidate {:?} words",self.outcome,self.ir.expanded_sites(),self.ir.automatic_expanded_sites(),self.retained_sites,self.ir.introduced_nodes,self.fallback_count,self.baseline_words,self.candidate_words)?;
        writeln!(
            f,
            "  target lowering: baseline {:?}, candidate {:?}",
            self.baseline_lowering, self.candidate_lowering
        )?;
        for cost in &self.frames {
            writeln!(
                f,
                "  {}: frame {} -> {}, GPR spill {} -> {}, FPU spill area {} -> {} words",
                cost.function,
                cost.baseline_frame,
                cost.candidate_frame,
                cost.baseline_gpr_spill,
                cost.candidate_gpr_spill,
                cost.baseline_fpu_spill,
                cost.candidate_fpu_spill
            )?;
            if let Some(reason) = cost.rejection() {
                writeln!(f, "    caller restored: {reason:?}")?;
            }
        }
        for (index, site) in self.ir.sites.iter().enumerate() {
            let origin = self.origins.get(index);
            writeln!(f,"  {} -> {}: {:?}; automatic {}; depth {}; source {} {:?}:{:?}; callee {:?}; physical caller {:?}:{:?}",site.caller,site.callee,site.decision,site.automatic,site.depth,site.source_function,origin.and_then(|o|o.source_file.as_deref()),site.line,origin.and_then(|o|o.callee_file.as_deref()),origin.and_then(|o|o.caller_file.as_deref()),site.callsite_line)?;
        }
        Ok(())
    }
}
/// Compile from one checked source snapshot. Mutable IR, allocation, fixups and
/// debug products are owned independently by each target-lowering invocation.
pub fn try_compile_with_inline_report(
    program: rcc::frontend::Program,
    options: &CompilerOptions,
    main: FuncName,
) -> Result<(CpuV3Program, InlineCompilationReport), BackendError> {
    let debug = program.debug;
    let functions: HashMap<_, _> = program.funcs.into_iter().map(|f| (f.name, f)).collect();
    let mut report = InlineCompilationReport::default();
    if !options.inline.enabled || options.opt.is_disabled() {
        report.outcome = InlineOutcome::Disabled;
        return Ok((compile_ir(functions, options, main, debug)?, report));
    }
    let live = reachable_functions(&functions, main)?;
    if !options.inline.automatic
        && !live
            .iter()
            .any(|n| functions[n].inline_hint == InlineHint::Hint)
    {
        return Ok((compile_ir(functions, options, main, debug)?, report));
    }
    let mut candidate = functions.clone();
    report.ir = inline_calls(
        &mut candidate,
        &live,
        &HashSet::from([main]),
        &options.inline,
    )
    .map_err(BackendError::Inline)?;
    let file = |name| {
        debug
            .funcs
            .iter()
            .find(|d| d.name == name)
            .and_then(|d| debug.files.get(d.file as usize))
            .cloned()
    };
    report.origins = report
        .ir
        .sites
        .iter()
        .enumerate()
        .map(|(site, s)| InlineSourceOrigin {
            site,
            caller_file: file(s.caller),
            source_file: file(s.source_function),
            callee_file: file(s.callee),
        })
        .collect();
    if report.ir.expanded_sites() == 0 {
        return Ok((compile_ir(functions, options, main, debug)?, report));
    }
    let start = Instant::now();
    let (baseline, baseline_costs) =
        compile_ir_with_allocations(&functions, options, main, debug.clone())?;
    report.baseline_lowering = start.elapsed();
    report.baseline_words = Some(baseline.words.len());
    let start = Instant::now();
    let mut selection = InlineSelection {
        originals: &functions,
        baseline_costs: &baseline_costs,
        frames: Vec::new(),
    };
    let result = compile_ir_selected(&candidate, options, main, debug, Some(&mut selection));
    report.candidate_lowering = start.elapsed();
    let (candidate, _) = match result {
        Ok(value) => value,
        Err(error) if error.is_resource_limit() => {
            report.outcome = InlineOutcome::Baseline(InlineRejection::Resource(error.to_string()));
            report.fallback_count = 1;
            return Ok((baseline, report));
        }
        Err(error) => return Err(error),
    };
    report.candidate_words = Some(candidate.words.len());
    let changed: HashSet<_> = report
        .ir
        .sites
        .iter()
        .filter(|s| s.decision == InlineDecision::Expanded)
        .map(|s| s.caller)
        .collect();
    report.frames = selection
        .frames
        .into_iter()
        .filter(|frame| changed.contains(&frame.function))
        .collect();
    report.frames.sort_by_key(|frame| frame.function);
    let rejected: HashSet<_> = report
        .frames
        .iter()
        .filter(|frame| frame.rejection().is_some())
        .map(|frame| frame.function)
        .collect();
    let allowance = 64.max(baseline.words.len().div_ceil(10));
    let rejection = if candidate.words.len() > baseline.words.len() + allowance {
        Some(InlineRejection::CodeGrowth {
            baseline: baseline.words.len(),
            candidate: candidate.words.len(),
            allowance,
        })
    } else {
        None
    };
    if let Some(reason) = rejection {
        report.outcome = InlineOutcome::Baseline(reason);
        report.fallback_count = 1;
        Ok((baseline, report))
    } else {
        report.retained_sites = report
            .ir
            .sites
            .iter()
            .filter(|site| {
                site.decision == InlineDecision::Expanded && !rejected.contains(site.caller)
            })
            .count();
        report.outcome = InlineOutcome::Applied;
        Ok((candidate, report))
    }
}

#[cfg(test)]
#[path = "inline_tests.rs"]
mod tests;
