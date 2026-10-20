mod attachment;
mod binding;
mod contract;
mod verify;

pub use attachment::{EvidenceAttachment, EvidenceAttachmentAdapter, EvidenceAttachmentKind};
pub use binding::{InputBinding, InputBindingDigests, INPUT_BINDING_SCHEMA_VERSION};
pub use contract::{ContractKind, InputContract, VerifiedContract};
pub use verify::{InputError, InputVerificationRequest, InputVerifier, ReviewInput};

#[cfg(test)]
mod tests;
