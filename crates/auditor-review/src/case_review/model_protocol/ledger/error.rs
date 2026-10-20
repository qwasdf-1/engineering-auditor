use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskLedgerError {
    InvalidReference,
    DuplicateReference(String),
    UnknownReference(String),
    StaleRevision {
        reference: String,
        expected: u32,
        actual: u32,
    },
    EmptyRevisionReason,
    AlreadyActive(String),
    AlreadyWithdrawn(String),
    ImmutableField {
        reference: String,
        field: &'static str,
    },
    UnknownTask {
        kind: &'static str,
        id: String,
    },
    TaskAlreadyCovered(String),
    TaskAlreadyDeclined(String),
    EmptyReason,
    IncompleteTasks(Vec<String>),
    InvalidComponent(String),
}

impl fmt::Display for TaskLedgerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(result) = self.fmt_record_error(formatter) {
            return result;
        }
        self.fmt_task_error(formatter)
    }
}

impl TaskLedgerError {
    fn fmt_record_error(&self, formatter: &mut fmt::Formatter<'_>) -> Option<fmt::Result> {
        match self {
            Self::InvalidReference => Some(formatter.write_str("task reference must not be empty")),
            Self::DuplicateReference(reference) => {
                Some(fmt_reference_state(formatter, reference, "already exists"))
            }
            Self::UnknownReference(reference) => {
                Some(fmt_reference_state(formatter, reference, "does not exist"))
            }
            Self::StaleRevision {
                reference,
                expected,
                actual,
            } => Some(write!(
                formatter,
                "task reference `{reference}` expected revision {expected}, current revision is {actual}"
            )),
            Self::EmptyRevisionReason => {
                Some(formatter.write_str("revision reason must not be empty"))
            }
            Self::AlreadyActive(reference) => {
                Some(fmt_reference_state(formatter, reference, "is already active"))
            }
            Self::AlreadyWithdrawn(reference) => Some(fmt_reference_state(
                formatter,
                reference,
                "is already withdrawn",
            )),
            Self::ImmutableField { reference, field } => Some(write!(
                formatter,
                "task reference `{reference}` cannot change `{field}`"
            )),
            _ => None,
        }
    }

    fn fmt_task_error(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTask { kind, id } => write!(formatter, "unknown {kind} `{id}`"),
            Self::TaskAlreadyCovered(id) => write!(formatter, "task `{id}` is already covered"),
            Self::TaskAlreadyDeclined(id) => write!(formatter, "task `{id}` is already declined"),
            Self::EmptyReason => formatter.write_str("task reason must not be empty"),
            Self::IncompleteTasks(tasks) => {
                write!(formatter, "task ledger is incomplete: {}", tasks.join(", "))
            }
            Self::InvalidComponent(message) => formatter.write_str(message),
            _ => formatter.write_str("task ledger record error"),
        }
    }
}

impl std::error::Error for TaskLedgerError {}

fn fmt_reference_state(
    formatter: &mut fmt::Formatter<'_>,
    reference: &str,
    state: &str,
) -> fmt::Result {
    write!(formatter, "task reference `{reference}` {state}")
}
