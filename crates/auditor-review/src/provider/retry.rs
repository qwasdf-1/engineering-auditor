use std::time::Duration;

/// Quick retries of a request on one target after a failure with no model
/// output, before the next target takes over.
pub(crate) const MAX_NO_MODEL_OUTPUT_RETRIES: u8 = 4;
/// Further resends on the last target, which has no target after it: an
/// outage is waited out rather than ending the review.
pub(crate) const OUTAGE_RESENDS: u8 = 6;

/// Hosted providers' rate limits and outages last minutes, not seconds: the
/// quick retries wait 15, 30, 60 and 120 s, and each outage resend waits 5
/// minutes, so the last target rides out about 34 minutes of waiting plus
/// the requests' own time (an hour when every request hangs to its timeout).
const NO_MODEL_OUTPUT_RETRY_BASE_DELAY_MILLIS: u64 = 15_000;
const NO_MODEL_OUTPUT_RETRY_MAX_DELAY_MILLIS: u64 = 120_000;
const OUTAGE_RESEND_DELAY_MILLIS: u64 = 300_000;

/// The wait before retry `retry_number` (0-based) of a request.
pub(crate) fn no_model_output_retry_delay(retry_number: u8) -> Duration {
    if retry_number >= MAX_NO_MODEL_OUTPUT_RETRIES {
        return Duration::from_millis(OUTAGE_RESEND_DELAY_MILLIS);
    }
    let multiplier = 1_u64 << u32::from(retry_number);
    Duration::from_millis(
        NO_MODEL_OUTPUT_RETRY_BASE_DELAY_MILLIS
            .saturating_mul(multiplier)
            .min(NO_MODEL_OUTPUT_RETRY_MAX_DELAY_MILLIS),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_retries_back_off_then_outage_resends_wait_five_minutes() {
        let delays = (0..MAX_NO_MODEL_OUTPUT_RETRIES + OUTAGE_RESENDS)
            .map(|retry| no_model_output_retry_delay(retry).as_secs())
            .collect::<Vec<_>>();
        assert_eq!(delays, [15, 30, 60, 120, 300, 300, 300, 300, 300, 300]);
        assert_eq!(no_model_output_retry_delay(u8::MAX).as_secs(), 300);
    }
}
