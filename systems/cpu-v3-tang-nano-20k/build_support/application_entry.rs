//! Keep RCC's real main/static initialization when selecting a diagnostic entry.
pub fn select(source: &str, entry: &str) -> Result<String, String> {
    if entry == "main" {
        return Ok(source.to_owned());
    }
    syn::parse_str::<syn::Ident>(entry)
        .map_err(|error| format!("invalid application entry: {error}"))?;
    let file = syn::parse_file(source).map_err(|error| format!("application syntax: {error}"))?;
    let functions: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "main" => Some(function),
            _ => None,
        })
        .collect();
    if functions.len() != 1 {
        return Err("explicit application needs exactly one main".into());
    }
    let span = functions[0].block.brace_token.span;
    let offset = |line: usize, column: usize| {
        source
            .split_inclusive('\n')
            .take(line - 1)
            .map(str::len)
            .sum::<usize>()
            + column
    };
    let start = offset(span.start().line, span.start().column);
    let end = offset(span.end().line, span.end().column);
    if !source
        .get(start..end)
        .is_some_and(|body| body.starts_with('{') && body.ends_with('}'))
    {
        return Err("main body source span is invalid".into());
    }
    let mut output = source.to_owned();
    output.replace_range(start..end, &format!("{{ {entry}(); }}"));
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_entry_preserves_source_and_main_initialization_site() {
        let source = "// UTF-8: 测试\r\nfn main(){run(0);}\r\nfn selected(){run(3);}\r\n";
        assert_eq!(select(source, "main").unwrap(), source);
        assert_eq!(
            select(source, "selected").unwrap(),
            source.replace("{run(0);}", "{ selected(); }")
        );
        assert!(select("fn other(){}", "selected").is_err());
        assert!(select(source, "selected();halt(1)").is_err());
    }
}
