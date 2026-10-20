//! Findings for an operation reached through repository calls.

use auditor_core::{Confidence, Finding};

/// `finding` for an operation reached through `via`, the called functions as
/// `name (path:line)` joined by ` -> `. Each resolved call is one more step
/// from the call site, so the message names the first called function, the
/// confidence drops to Medium, and `via` evidence holds the chain. Without
/// `via` the finding is unchanged.
pub(crate) fn through_calls(mut finding: Finding, via: Option<&str>) -> Finding {
    let Some(via) = via else {
        return finding;
    };
    finding.message = format!(
        "{}, through `{}`.",
        finding.message.trim_end_matches('.'),
        via.split(" (").next().unwrap_or(via)
    );
    finding.confidence = Confidence::Medium;
    finding.with_evidence("via", via.to_string())
}
