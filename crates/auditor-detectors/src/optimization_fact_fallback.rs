//! Eager fallback arguments: Rust `unwrap_or`/`ok_or`/`or`, Java
//! `Optional.orElse`, and C# `GetValueOrDefault` given a value built whether
//! or not it is needed.

use super::collectors::{pure_proof, push_fact};
use super::flow::calls;
use super::language_forms::java_pure_eager_fallback;
use super::operations::pure_eager_fallback;
use super::syntax::{call_name, strip_call_delimiters};
use super::{FunctionView, OptimizationFact, OptimizationFactKind};
use crate::Language;

pub(super) fn collect_eager_fallback(view: &FunctionView<'_>, facts: &mut Vec<OptimizationFact>) {
    if !matches!(
        view.file.language,
        Language::Rust | Language::Java | Language::CSharp
    ) {
        return;
    }
    for call in calls(view) {
        let api = call_name(call);
        let argument = call
            .arguments
            .as_deref()
            .map(|arguments| strip_call_delimiters(arguments).trim().to_string());
        let fallback = match view.file.language {
            Language::CSharp => super::csharp::csharp_eager_fallback(call),
            Language::Java => argument.filter(|fallback| {
                call.api.as_deref() == Some("java.util.Optional.orElse")
                    && java_pure_eager_fallback(fallback)
            }),
            _ => argument.filter(|fallback| {
                matches!(api.as_str(), "unwrap_or" | "ok_or" | "or")
                    && pure_eager_fallback(fallback)
            }),
        };
        let Some(fallback) = fallback else {
            continue;
        };
        push_fact(
            view,
            call.line,
            OptimizationFactKind::EagerFallback { api, fallback },
            pure_proof(
                "lazy_fallback_only",
                "same_fallback_failure_when_needed",
                "option_or_result",
            ),
            facts,
        );
    }
}
