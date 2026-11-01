//! All selected command owners now express diagnostics in their own sources.
//! Preserve the existing factory API and line-ending normalization until its
//! redundant call boundary is removed; no RTL scanning or policy rewrite remains.
pub fn lower(source: String) -> Result<String, String> {
    Ok(source.replace("\r\n", "\n"))
}
