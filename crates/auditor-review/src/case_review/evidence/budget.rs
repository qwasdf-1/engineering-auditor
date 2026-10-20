use serde::{Deserialize, Serialize};

use crate::{LookupBudget, ReviewBrief};

use super::EvidenceError;

pub const TARGET_PACKET_BYTES: usize = 64 * 1024;
pub const HARD_PACKET_BYTES: usize = 128 * 1024;
pub const INITIAL_SOURCE_EXCERPTS: usize = 8;
pub const ABSOLUTE_SOURCE_EXCERPTS: usize = 16;
pub const MAXIMUM_GRAPH_HOPS: usize = 2;
pub const EVIDENCE_UNION_ROUNDS: usize = 1;

const MINIMUM_HARD_PACKET_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencePacketBudget {
    pub target_packet_bytes: usize,
    pub hard_packet_bytes: usize,
    pub initial_source_excerpts: usize,
    pub absolute_source_excerpts: usize,
    pub maximum_graph_hops: usize,
    pub evidence_union_rounds: usize,
    pub maximum_lookup_requests: usize,
}

impl Default for EvidencePacketBudget {
    fn default() -> Self {
        Self {
            target_packet_bytes: TARGET_PACKET_BYTES,
            hard_packet_bytes: HARD_PACKET_BYTES,
            initial_source_excerpts: INITIAL_SOURCE_EXCERPTS,
            absolute_source_excerpts: ABSOLUTE_SOURCE_EXCERPTS,
            maximum_graph_hops: MAXIMUM_GRAPH_HOPS,
            evidence_union_rounds: EVIDENCE_UNION_ROUNDS,
            maximum_lookup_requests: INITIAL_SOURCE_EXCERPTS,
        }
    }
}

impl EvidencePacketBudget {
    pub fn tightened_by(mut self, brief: Option<&ReviewBrief>) -> Result<Self, EvidenceError> {
        if let Some(brief) = brief {
            if let Some(bytes) = brief.budgets.max_context_bytes {
                self.target_packet_bytes = self.target_packet_bytes.min(bytes);
                self.hard_packet_bytes = self.hard_packet_bytes.min(bytes);
            }
            if let Some(requests) = brief.budgets.max_lookup_requests {
                self.maximum_lookup_requests = self.maximum_lookup_requests.min(requests);
                self.initial_source_excerpts = self.initial_source_excerpts.min(requests);
                self.absolute_source_excerpts = self.absolute_source_excerpts.min(requests);
            }
            if let Some(hops) = brief.budgets.max_lookup_hops {
                self.maximum_graph_hops = self.maximum_graph_hops.min(hops);
            }
        }
        self.validate()?;
        Ok(self)
    }

    pub fn validate(self) -> Result<(), EvidenceError> {
        if self.target_packet_bytes == 0
            || self.hard_packet_bytes < MINIMUM_HARD_PACKET_BYTES
            || self.target_packet_bytes > self.hard_packet_bytes
            || self.hard_packet_bytes > HARD_PACKET_BYTES
            || self.initial_source_excerpts == 0
            || self.initial_source_excerpts > self.absolute_source_excerpts
            || self.absolute_source_excerpts > ABSOLUTE_SOURCE_EXCERPTS
            || self.maximum_graph_hops > MAXIMUM_GRAPH_HOPS
            || self.evidence_union_rounds > EVIDENCE_UNION_ROUNDS
            || self.maximum_lookup_requests == 0
        {
            return Err(EvidenceError::InvalidBudget(
                "values are empty, inconsistent, or exceed runtime hard ceilings".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn lookup_budget(self) -> LookupBudget {
        LookupBudget {
            max_requests: self.maximum_lookup_requests,
            max_excerpts: self.absolute_source_excerpts,
            max_excerpt_bytes: self.hard_packet_bytes,
            max_total_bytes: self.hard_packet_bytes,
            max_hops: self.maximum_graph_hops,
        }
    }
}
