//! Loops whose passes each handle a group of items.

use super::{StructureComprehension, StructureLoop};
use crate::util;

/// Last words of a loop binding that name a group of items.
const BATCH_WORDS: &[&str] = &["batch", "chunk", "page", "window", "partition", "shard"];

/// Calls that split a collection or a result set into groups.
const BATCHING_CALLS: &[&str] = &[
    "chunks",
    "chunks_exact",
    "chunked",
    "chunk",
    "batched",
    "batches",
    "partition",
    "windows",
    "paginate",
];

impl StructureLoop {
    /// Each pass handles a group of items (a batch, chunk, page, or window),
    /// so an operation per pass is already the batched form.
    pub(crate) fn walks_batches(&self) -> bool {
        walks_batches(&self.binding, self.iterable.as_deref())
    }
}

impl StructureComprehension {
    /// Each item is a group of items, as for [`StructureLoop::walks_batches`].
    pub(crate) fn walks_batches(&self) -> bool {
        walks_batches(&self.binding, self.iterable.as_deref())
    }
}

fn walks_batches(binding: &[String], iterable: Option<&str>) -> bool {
    let batch_binding = binding.iter().any(|binding| {
        util::split_identifier(binding)
            .last()
            .is_some_and(|word| BATCH_WORDS.contains(&word.as_str()))
    });
    let batching_iterable = iterable.is_some_and(|iterable| {
        BATCHING_CALLS.iter().any(|name| {
            iterable
                .match_indices(name)
                .any(|(index, _)| called_name_at(iterable, index, name.len()))
        })
    });
    batch_binding || batching_iterable
}

/// `text[index..index + length]` is a whole name followed by a call.
fn called_name_at(text: &str, index: usize, length: usize) -> bool {
    let before = text[..index].chars().next_back();
    let after = text[index + length..].trim_start().chars().next();
    !before.is_some_and(|character| character.is_alphanumeric() || character == '_')
        && after == Some('(')
}
