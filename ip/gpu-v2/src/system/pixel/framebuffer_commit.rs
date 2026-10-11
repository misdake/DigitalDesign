//! Fold the serial post-write notification into the actual depth-write edge.
use super::replace_once;

pub(super) fn lower(mut source: String) -> Result<String, String> {
    let start = source
        .find("                        RS_WRDEP: begin")
        .ok_or("Missing depth-write state")?;
    let notification = source
        .find("                        RS_DONE: begin\n")
        .ok_or("Missing commit state")?;
    let end = notification
        + source[notification..]
            .find("                        default: rst <= RS_ROWS;")
            .ok_or("Missing commit end")?;
    let transition = start
        + source[start..notification]
            .find("                            rst <= RS_DONE;")
            .ok_or("Missing depth-write transition")?;
    let body = notification + "                        RS_DONE: begin\n".len();
    let replacement=format!("{}                            // This edge writes depth with the old owner; its\n                            // registered notification is observed after the write.\n                            // A newly detected protocol fault cannot publish it.\n                            if (!ingress_error && !extra_read) begin\n{}                        end\n", &source[start..transition], &source[body..end]);
    let anchor = source[start..end].to_owned();
    replace_once(&mut source, &anchor, &replacement)?;
    Ok(source)
}
