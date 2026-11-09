//! The review's progress log: a line as each question starts and ends,
//! before each retry, at each target change, and after each slow request.

use std::time::Duration;

use super::record::QuestionRecord;
use crate::{QuestionKind, RouteTarget};

/// A request that takes this long is reported.
pub(super) const SLOW_REQUEST_SECONDS: u64 = 60;
/// How much of a failure's message a line keeps.
const REPORTED_FAILURE_CHARS: usize = 200;

#[derive(Default)]
pub(super) struct Progress<'a> {
    sink: Option<&'a dyn Fn(&str)>,
    /// The question being asked (`question 2 of 8`), which each line names.
    place: String,
}

impl<'a> Progress<'a> {
    pub(super) fn new(sink: Option<&'a dyn Fn(&str)>) -> Self {
        Self {
            sink,
            place: String::new(),
        }
    }

    fn report(&self, line: impl FnOnce() -> String) {
        if let Some(sink) = self.sink {
            sink(&format!("{}{}", self.place, line()));
        }
    }

    /// A question about to be asked, or passed over because the review
    /// already stopped.
    pub(super) fn question_started(
        &mut self,
        (number, of): (usize, usize),
        kind: QuestionKind,
        stopped: bool,
    ) {
        self.place = format!("question {number} of {of}");
        if stopped {
            self.report(|| format!(" ({kind:?}) not asked: the review stopped"));
        } else {
            self.report(|| format!(" ({kind:?}) started"));
        }
    }

    pub(super) fn question_ended(&self, record: &QuestionRecord) {
        self.report(|| format!(" {:?} after {} requests", record.status, record.requests));
    }

    pub(super) fn retry(
        &self,
        request: &str,
        failure: &str,
        (retry, retries): (u8, u8),
        delay: Duration,
    ) {
        self.report(|| {
            let failure = failure.chars().take(REPORTED_FAILURE_CHARS);
            format!(
                ": request {} failed ({}); retry {retry} of {retries} in {} s",
                number(request),
                failure.collect::<String>(),
                delay.as_secs()
            )
        });
    }

    pub(super) fn slow(&self, request: &str, target: &RouteTarget, seconds: u64) {
        self.report(|| {
            format!(
                ": request {} on {}/{} took {seconds} s",
                number(request),
                target.provider_id,
                target.model
            )
        });
    }

    pub(super) fn target_changed(&self, failed: &RouteTarget, next: &RouteTarget) {
        self.report(|| {
            format!(
                ": {}/{} failed for good; {}/{} takes over",
                failed.provider_id, failed.model, next.provider_id, next.model
            )
        });
    }
}

/// A request's place in its question (`#3`) from its ID.
fn number(request_id: &str) -> &str {
    request_id
        .rfind('#')
        .map_or(request_id, |at| &request_id[at..])
}
