//! Source-aware RCC frontend diagnostics.

use std::fmt;

/// A source-aware compiler diagnostic. Unlike `syn::Error::to_string()`, its
/// display includes the source file, one-based line/column, source text, and a
/// caret. Full-program compilation uses this type so errors from module files
/// retain their origin.
#[derive(Clone, Debug)]
pub struct CompileError {
    diagnostics: Vec<SourceDiagnostic>,
}

#[derive(Clone, Debug)]
struct SourceDiagnostic {
    file: String,
    source: String,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
    message: String,
}

impl CompileError {
    pub(super) fn from_syn(
        file: impl Into<String>,
        source: impl Into<String>,
        error: syn::Error,
    ) -> Self {
        let file = file.into();
        let source = source.into();
        let diagnostics = error
            .into_iter()
            .map(|error| {
                let start = error.span().start();
                let end = error.span().end();
                SourceDiagnostic {
                    file: file.clone(),
                    source: source.clone(),
                    line: start.line,
                    column: start.column + 1,
                    end_line: end.line,
                    end_column: end.column + 1,
                    message: error.to_string(),
                }
            })
            .collect();
        Self { diagnostics }
    }

    /// Location of the primary diagnostic as `(file, one-based line, column)`.
    pub fn location(&self) -> Option<(&str, usize, usize)> {
        self.diagnostics
            .first()
            .map(|diagnostic| (diagnostic.file.as_str(), diagnostic.line, diagnostic.column))
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index != 0 {
                writeln!(f)?;
            }
            writeln!(f, "error: {}", diagnostic.message)?;
            writeln!(
                f,
                " --> {}:{}:{}",
                diagnostic.file, diagnostic.line, diagnostic.column
            )?;
            if let Some(line) = diagnostic
                .source
                .lines()
                .nth(diagnostic.line.saturating_sub(1))
            {
                let number_width = diagnostic.line.to_string().len();
                writeln!(f, "{space:>width$} |", space = "", width = number_width)?;
                writeln!(f, "{} | {}", diagnostic.line, line)?;
                let caret_width = if diagnostic.end_line == diagnostic.line {
                    diagnostic
                        .end_column
                        .saturating_sub(diagnostic.column)
                        .max(1)
                } else {
                    1
                };
                writeln!(
                    f,
                    "{space:>width$} | {padding}{carets}",
                    space = "",
                    width = number_width,
                    padding = " ".repeat(diagnostic.column.saturating_sub(1)),
                    carets = "^".repeat(caret_width),
                )?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for CompileError {}
