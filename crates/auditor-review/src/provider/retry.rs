use std::time::Duration;

/// Four retries (five total attempts) is the v2 provider recovery contract.
/// Keep qualification and production on this shared ceiling so a route cannot
/// qualify under a transient sequence that production would reject.
pub(crate) const MAX_NO_MODEL_OUTPUT_RETRIES: u8 = 4;

const NO_MODEL_OUTPUT_RETRY_BASE_DELAY_MILLIS: u64 = 500;
const NO_MODEL_OUTPUT_RETRY_MAX_DELAY_MILLIS: u64 = 4_000;

pub(crate) fn no_model_output_retry_delay(retry_number: u8) -> Duration {
    let multiplier = 1_u64 << u32::from(retry_number.min(MAX_NO_MODEL_OUTPUT_RETRIES - 1));
    Duration::from_millis(
        NO_MODEL_OUTPUT_RETRY_BASE_DELAY_MILLIS
            .saturating_mul(multiplier)
            .min(NO_MODEL_OUTPUT_RETRY_MAX_DELAY_MILLIS),
    )
}

pub(crate) fn wait_before_no_model_output_retry(retry_number: u8) {
    std::thread::sleep(no_model_output_retry_delay(retry_number));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_is_bounded_exponential_backoff() {
        const BASE_DELAY: Duration = Duration::from_millis(NO_MODEL_OUTPUT_RETRY_BASE_DELAY_MILLIS);
        const MAX_DELAY: Duration = Duration::from_millis(NO_MODEL_OUTPUT_RETRY_MAX_DELAY_MILLIS);
        assert_eq!(no_model_output_retry_delay(0), Duration::from_millis(500));
        assert_eq!(no_model_output_retry_delay(1), Duration::from_secs(1));
        assert_eq!(no_model_output_retry_delay(2), Duration::from_secs(2));
        assert_eq!(BASE_DELAY, Duration::from_millis(500));
        assert_eq!(no_model_output_retry_delay(3), MAX_DELAY);
        assert_eq!(
            no_model_output_retry_delay(MAX_NO_MODEL_OUTPUT_RETRIES),
            MAX_DELAY
        );
        assert_eq!(no_model_output_retry_delay(u8::MAX), MAX_DELAY);
    }
}
