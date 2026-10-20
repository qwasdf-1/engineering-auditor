//! Shapes the clone detector does not report: tokens that state an
//! interface or document rather than behave, and clone classes whose
//! repetition is a convention (sync/async twins, enum state machines, locale
//! files, row tables).

use super::*;

/// A string statement opening a Python function, class, or module body:
/// documentation, which would otherwise count its lines toward a clone.
pub(super) fn is_python_docstring(node: AstNode<'_>, language: &Language) -> bool {
    *language == Language::Python
        && node.kind() == "expression_statement"
        && node.named_child_count() == 1
        && node
            .named_child(0)
            .is_some_and(|child| child.kind() == "string")
        && node.parent().is_some_and(|parent| {
            matches!(parent.kind(), "block" | "module")
                && parent
                    .named_child(0)
                    .is_some_and(|first| first.start_byte() == node.start_byte())
        })
}

pub(super) fn is_declarative_field_mapping(node: AstNode<'_>, language: &Language) -> bool {
    *language == Language::Rust && node.kind() == "field_initializer_list"
}

/// A clone that pairs async functions with their sync twins (`send` and
/// `async send`, `close` and `aclose`, `handle_request` and
/// `handle_async_request`, `__enter__` and `__aenter__`): Python and
/// JavaScript cannot share a body across the two without code generation.
pub(super) fn is_sync_async_twin(files: &[TokenizedFile], class: &CloneClass) -> bool {
    let mut sync = BTreeSet::new();
    let mut asynchronous = BTreeSet::new();
    for occurrence in &class.occurrences {
        let file = &files[occurrence.file];
        let Some(owner) = file.tokens[occurrence.start]
            .function_owner
            .and_then(|owner| file.owners.get(&owner))
        else {
            return false;
        };
        if owner.is_async {
            asynchronous.insert(owner.name.as_str());
        } else {
            sync.insert(owner.name.as_str());
        }
    }
    let twin = |name: &str| {
        sync.contains(name)
            || name
                .strip_prefix('a')
                .is_some_and(|rest| sync.contains(rest))
            || sync.contains(name.replacen("async_", "", 1).as_str())
            || sync.contains(name.replacen("_async", "", 1).as_str())
            || sync.contains(name.replacen("__a", "__", 1).as_str())
    };
    !sync.is_empty() && !asynchronous.is_empty() && asynchronous.iter().all(|name| twin(name))
}

/// Handlers of different states of one enum state machine (jsoup's
/// `TokeniserState`): each state transcribes its own section of a
/// specification, which repeats steps across states by design.
pub(super) fn is_state_machine_repetition(files: &[TokenizedFile], class: &CloneClass) -> bool {
    let mut states = BTreeSet::new();
    let mut handlers = BTreeSet::new();
    for occurrence in &class.occurrences {
        let file = &files[occurrence.file];
        let Some((owner, state_of)) = file.tokens[occurrence.start]
            .function_owner
            .and_then(|owner| Some((owner, file.owners.get(&owner)?.state_of?)))
        else {
            return false;
        };
        states.insert((occurrence.file, state_of));
        handlers.insert((occurrence.file, owner));
    }
    states.len() == 1 && handlers.len() == class.occurrences.len()
}

/// A clone among translation files (`locales/de.ts`, `locales/fr.ts`).
pub(super) fn is_locale_repetition(files: &[TokenizedFile], class: &CloneClass) -> bool {
    let paths = class
        .occurrences
        .iter()
        .map(|occurrence| files[occurrence.file].path.as_str())
        .collect::<BTreeSet<_>>();
    paths.len() > 1 && paths.iter().all(|path| util::is_locale_path(path))
}

/// Code that states an interface rather than behavior, so repeating it is not
/// extractable duplication: a function's parameter list (explicit keyword
/// APIs repeat it by design), a keyword argument forwarding a parameter of the
/// same name (`params=params`), and an argument list of literals only (a data
/// row such as `table.add_row("--help", "Show this message.")`).
pub(super) fn is_declarative_call_shape(node: AstNode<'_>) -> bool {
    match node.kind() {
        "parameters" | "formal_parameters" | "parameter_list" => node
            .parent()
            .is_some_and(|parent| auditor_ast::function_node_kinds().contains(&parent.kind())),
        "keyword_argument" => node
            .child_by_field_name("name")
            .zip(node.child_by_field_name("value"))
            .is_some_and(|(name, value)| name.text() == value.text()),
        "argument_list" | "arguments" => {
            let mut arguments = node.named_children().peekable();
            arguments.peek().is_some()
                && arguments.all(|argument| {
                    classify_token(argument.kind(), argument.text().trim()) == TokenKind::Literal
                })
        }
        _ => false,
    }
}

pub(super) fn is_declarative_json_macro(node: AstNode<'_>) -> bool {
    if node.kind() != "macro_invocation" {
        return false;
    }
    node.text()
        .split_once('!')
        .map(|(name, _)| name.trim().ends_with("json"))
        .unwrap_or(false)
}

/// Two occurrences back to back in one function are halves of one row
/// table (match arms `A, B` against `C, D`) when the two together repeat a
/// row shape at least three times.
pub(super) fn abuts_as_rows(files: &[TokenizedFile], class: &CloneClass) -> bool {
    const MAX_GAP: usize = 3;
    let occurrences = class.occurrences.iter().collect::<Vec<_>>();
    occurrences.windows(2).any(|pair| {
        let (first, second) = (pair[0], pair[1]);
        let tokens = &files[first.file].tokens;
        first.file == second.file
            && second.start <= first.end + MAX_GAP
            && tokens[first.start].function_owner == tokens[second.start].function_owner
            && is_parallel_rows(&tokens[first.start..second.end])
    })
}

/// A table of parallel rows (match arms or statements repeating one shape
/// with different names): most of the occurrence is a single row shape seen
/// at least three times, so what repeats is the shape, not the logic.
pub(super) fn is_parallel_rows(tokens: &[DetectionToken]) -> bool {
    const MIN_ROW_TOKENS: usize = 4;
    (MIN_ROW_TOKENS..=tokens.len() / 3).any(|period| {
        let mut run = 0;
        let mut longest = 0;
        for index in 0..tokens.len() - period {
            if tokens[index].hash == tokens[index + period].hash {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
            }
        }
        longest >= 2 * period && (longest + period) * 4 >= tokens.len() * 3
    })
}
