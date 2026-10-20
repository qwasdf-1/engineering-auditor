pub(crate) use crate::security_crypto::insecure_cryptography_findings;
pub(crate) use crate::security_process::shell_execution_findings;
pub(crate) use crate::security_sql::sql_injection_findings;

#[cfg(test)]
use crate::{util, AuditedFile, AuditorConfig};
#[cfg(test)]
use auditor_core::Finding;

#[cfg(test)]
include!("security_tests.rs");
