//! Repository-local type facts: what a value is, from the declarations the
//! repository itself contains. An expression chain (`sessionId`,
//! `table.getObjectId()`, `Constants.Aliases.CheckBoxList`) resolves through
//! the enclosing function's locals and parameters, the enclosing class's
//! fields and its bases, class names for static members and nested
//! classes, and method return types. Declarations outside the repository
//! stay unknown, apart from a few standard members whose result is a number.
//! Findings-free.

use std::collections::{BTreeMap, BTreeSet};

use auditor_ast::SymbolKind;

use crate::structure_index::{StructureIndex, StructureSymbol};

/// A symbol: its file and its index in that file.
pub(crate) type SymbolRef = (usize, usize);

/// How far a chain follows class bases and member types.
const MAX_DEPTH: usize = 6;

/// Declared types whose values render as a number, a boolean, or a GUID.
pub(crate) const NUMERIC_TYPES: &[&str] = &[
    "int",
    "long",
    "short",
    "byte",
    "uint",
    "ulong",
    "ushort",
    "sbyte",
    "double",
    "float",
    "decimal",
    "bool",
    "boolean",
    "char",
    "Integer",
    "Long",
    "Short",
    "Byte",
    "Double",
    "Float",
    "Boolean",
    "BigInteger",
    "BigDecimal",
    "Guid",
    "UUID",
    "int8",
    "int16",
    "int32",
    "int64",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "float32",
    "float64",
    "number",
    "Int",
    "Number",
    "usize",
    "isize",
    "i32",
    "i64",
    "u32",
    "u64",
];

/// Standard members whose result is a number whatever their receiver
/// (`name.length()`, `items.size()`, `list.Count`).
const NUMERIC_MEMBERS: &[&str] = &[
    "length",
    "size",
    "count",
    "Count",
    "Length",
    "ordinal",
    "hashCode",
    "GetHashCode",
];

/// Members of an enum value that render one of its constant names or a
/// number (`kind.name()`, `kind.ToString()`).
const ENUM_MEMBERS: &[&str] = &[
    "name", "toString", "ToString", "ordinal", "value", "getValue",
];

/// What an expression is known to be.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Resolution {
    /// The declared type's simple name.
    pub(crate) type_name: Option<String>,
    /// The initializer of the immutable member it reads (`"x"`, `List.of(..)`).
    pub(crate) constant: Option<String>,
    /// A constant of a repository enum, or a member of one that renders its
    /// name or ordinal.
    pub(crate) enum_value: bool,
}

impl Resolution {
    /// A number, a boolean, a GUID, or an enum's name: no SQL text fits.
    pub(crate) fn is_numeric_or_enum(&self) -> bool {
        self.enum_value
            || self
                .type_name
                .as_deref()
                .is_some_and(|name| NUMERIC_TYPES.contains(&name))
    }
}

/// The current step of a chain: a value of a type, or a type itself (for
/// static members and nested classes).
#[derive(Clone)]
enum Step {
    Value(Resolution),
    Types(Vec<SymbolRef>),
}

/// The type facts in force in one file, or none (a test, or a scan without
/// the structure index).
#[derive(Clone, Copy, Default)]
pub(crate) struct TypeScope<'f> {
    facts: Option<&'f TypeFacts<'f>>,
    file: usize,
}

impl<'f> TypeScope<'f> {
    pub(crate) fn of(facts: &'f TypeFacts<'f>, path: &str) -> Self {
        match facts.file_index(path) {
            Some(file) => Self {
                facts: Some(facts),
                file,
            },
            None => Self::default(),
        }
    }

    /// What `node` is where it is read.
    pub(crate) fn resolve(&self, node: auditor_ast::AstNode<'_>) -> Resolution {
        let Some(facts) = self.facts else {
            return Resolution::default();
        };
        let owner = facts.owner_at(self.file, node.start_byte());
        facts.resolve(self.file, owner, node.text())
    }

    /// The repository methods `receiver.method(..)` in function `owner` may run.
    pub(crate) fn methods(&self, owner: usize, receiver: &str, method: &str) -> Vec<SymbolRef> {
        match self.facts {
            Some(facts) => facts.methods(self.file, Some(owner), receiver, method),
            None => Vec::new(),
        }
    }

    /// What `expression` is when read inside function `owner`.
    pub(crate) fn resolve_text(&self, owner: usize, expression: &str) -> Resolution {
        match self.facts {
            Some(facts) => facts.resolve(self.file, Some(owner), expression),
            None => Resolution::default(),
        }
    }
}

/// One file's members by owner symbol: (parent, name) -> symbols.
type FileMembers<'i> = BTreeMap<(Option<u32>, &'i str), Vec<usize>>;

pub(crate) struct TypeFacts<'i> {
    index: &'i StructureIndex,
    /// Classes, interfaces, structs, enums, and objects by simple name.
    types: BTreeMap<&'i str, Vec<SymbolRef>>,
    /// Each file's members by owner symbol.
    members: Vec<FileMembers<'i>>,
    files_by_path: BTreeMap<&'i str, usize>,
}

impl<'i> TypeFacts<'i> {
    pub(crate) fn new(index: &'i StructureIndex) -> Self {
        let mut types: BTreeMap<&str, Vec<SymbolRef>> = BTreeMap::new();
        let mut members = Vec::with_capacity(index.files().len());
        let mut files_by_path = BTreeMap::new();
        for (file_index, file) in index.files().iter().enumerate() {
            files_by_path.insert(file.path.as_str(), file_index);
            let mut by_owner: FileMembers = BTreeMap::new();
            for (symbol_index, symbol) in file.symbols.iter().enumerate() {
                if is_type(symbol.kind) {
                    types
                        .entry(symbol.name.as_str())
                        .or_default()
                        .push((file_index, symbol_index));
                }
                let name = symbol.typing.binding.as_deref().unwrap_or(&symbol.name);
                by_owner
                    .entry((symbol.parent, name))
                    .or_default()
                    .push(symbol_index);
            }
            members.push(by_owner);
        }
        Self {
            index,
            types,
            members,
            files_by_path,
        }
    }

    pub(crate) fn file_index(&self, path: &str) -> Option<usize> {
        self.files_by_path.get(path).copied()
    }

    /// The innermost function of `file` containing byte `at`.
    pub(crate) fn owner_at(&self, file: usize, at: usize) -> Option<usize> {
        self.index.files()[file]
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| {
                matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
                    && symbol.span.start_byte <= at
                    && at < symbol.span.end_byte
            })
            .max_by_key(|(_, symbol)| symbol.span.start_byte)
            .map(|(index, _)| index)
    }

    /// What `expression` is when read inside function `owner` of `file`.
    pub(crate) fn resolve(
        &self,
        file: usize,
        owner: Option<usize>,
        expression: &str,
    ) -> Resolution {
        match self.resolve_step(file, owner, expression) {
            Some(Step::Value(resolution)) => resolution,
            _ => Resolution::default(),
        }
    }

    fn resolve_step(&self, file: usize, owner: Option<usize>, expression: &str) -> Option<Step> {
        let segments = chain_segments(expression);
        let (first, rest) = segments.split_first()?;
        let mut step = self.first_step(file, owner, first)?;
        for segment in rest {
            step = self.next_step(&step, segment)?;
        }
        Some(step)
    }

    /// The repository methods `receiver.method(..)`, called inside function
    /// `owner` of `file`, may run: the method each class the receiver may be
    /// declares or inherits. None when the receiver's type is unknown or a
    /// candidate class declares no such method.
    pub(crate) fn methods(
        &self,
        file: usize,
        owner: Option<usize>,
        receiver: &str,
        method: &str,
    ) -> Vec<SymbolRef> {
        let receiver = receiver.trim().trim_end_matches(['.', '?', ':']).trim();
        let classes = if matches!(receiver, "" | "this" | "self") {
            self.enclosing_class(file, owner).into_iter().collect()
        } else {
            match self.resolve_step(file, owner, receiver) {
                Some(Step::Types(classes)) => classes,
                Some(Step::Value(resolution)) => resolution
                    .type_name
                    .and_then(|name| self.types.get(name.as_str()).cloned())
                    .unwrap_or_default(),
                None => Vec::new(),
            }
        };
        classes
            .iter()
            .map(|class| self.class_method(*class, method, 0))
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default()
    }

    /// The method `class` declares or inherits through its one base.
    fn class_method(&self, class: SymbolRef, method: &str, depth: usize) -> Option<SymbolRef> {
        if depth > MAX_DEPTH {
            return None;
        }
        let (file, symbol) = class;
        let declared = self.members[file]
            .get(&(Some(symbol as u32), method))
            .and_then(|symbols| {
                symbols.iter().find(|candidate| {
                    is_callable(self.index.files()[file].symbols[**candidate].kind)
                })
            })
            .map(|candidate| (file, *candidate));
        declared.or_else(|| {
            let base = self.file_symbol(class).base.as_deref()?;
            let [base] = self.types.get(base)?.as_slice() else {
                return None;
            };
            self.class_method(*base, method, depth + 1)
        })
    }

    fn file_symbol(&self, (file, symbol): SymbolRef) -> &'i StructureSymbol {
        &self.index.files()[file].symbols[symbol]
    }

    fn first_step(&self, file: usize, owner: Option<usize>, segment: &Segment<'_>) -> Option<Step> {
        if matches!(segment.name, "this" | "self") && !segment.call {
            return self
                .enclosing_class(file, owner)
                .map(|class| Step::Types(vec![class]));
        }
        // A local or parameter of the function, then a member of its class,
        // then a top-level declaration of the file, then a type name.
        let local = owner.and_then(|owner| self.member(file, Some(owner as u32), segment));
        local
            .or_else(|| {
                let class = self.enclosing_class(file, owner)?;
                self.class_member(class, segment, 0)
            })
            .or_else(|| self.member(file, None, segment))
            .or_else(|| {
                (!segment.call)
                    .then(|| self.types.get(segment.name).cloned())
                    .flatten()
                    .map(|candidates| Step::Types(prefer_file(candidates, file)))
            })
    }

    fn next_step(&self, step: &Step, segment: &Segment<'_>) -> Option<Step> {
        match step {
            Step::Types(classes) => {
                self.across(classes, |class| self.class_member(class, segment, 0))
            }
            Step::Value(resolution) => {
                if resolution.enum_value && ENUM_MEMBERS.contains(&segment.name) {
                    return Some(Step::Value(Resolution {
                        enum_value: true,
                        ..Resolution::default()
                    }));
                }
                if NUMERIC_MEMBERS.contains(&segment.name) {
                    return Some(Step::Value(Resolution {
                        type_name: Some("int".to_string()),
                        ..Resolution::default()
                    }));
                }
                let classes = self.types.get(resolution.type_name.as_deref()?)?.clone();
                self.across(&classes, |class| self.class_member(class, segment, 0))
            }
        }
    }

    /// The one step every candidate class agrees on.
    fn across(
        &self,
        classes: &[SymbolRef],
        step: impl Fn(SymbolRef) -> Option<Step>,
    ) -> Option<Step> {
        let mut steps = classes.iter().map(|class| step(*class));
        let first = steps.next()??;
        steps
            .all(|other| other.is_some_and(|other| same_step(&first, &other)))
            .then_some(first)
    }

    /// The declaration `segment` names among the members of `parent` in `file`.
    fn member(&self, file: usize, parent: Option<u32>, segment: &Segment<'_>) -> Option<Step> {
        let symbols = self.members[file].get(&(parent, segment.name))?;
        let symbol = symbols
            .iter()
            .map(|symbol| (file, *symbol))
            .find(|symbol| is_callable(self.file_symbol(*symbol).kind) == segment.call)?;
        Some(self.step_of(symbol))
    }

    fn class_member(&self, class: SymbolRef, segment: &Segment<'_>, depth: usize) -> Option<Step> {
        if depth > MAX_DEPTH {
            return None;
        }
        let (file, symbol) = class;
        let declared = self.file_symbol(class);
        if declared.kind == SymbolKind::Enum && !segment.call {
            // An enum constant is not a symbol; any other name read off the
            // enum type is one of its constants.
            if !self.members[file].contains_key(&(Some(symbol as u32), segment.name)) {
                return Some(Step::Value(Resolution {
                    type_name: Some(declared.name.clone()),
                    enum_value: true,
                    ..Resolution::default()
                }));
            }
        }
        self.member(file, Some(symbol as u32), segment).or_else(|| {
            let base = declared.base.as_deref()?;
            let bases = self.types.get(base)?.clone();
            self.across(&bases, |base| self.class_member(base, segment, depth + 1))
        })
    }

    fn step_of(&self, symbol: SymbolRef) -> Step {
        let declared = self.file_symbol(symbol);
        if is_type(declared.kind) {
            return Step::Types(vec![symbol]);
        }
        let typing = &declared.typing;
        let enum_value = typing
            .type_name
            .as_deref()
            .and_then(|name| self.types.get(name))
            .is_some_and(|candidates| {
                candidates
                    .iter()
                    .all(|candidate| self.file_symbol(*candidate).kind == SymbolKind::Enum)
            });
        Step::Value(Resolution {
            type_name: typing.type_name.clone(),
            constant: typing
                .immutable
                .then(|| typing.initializer.clone())
                .flatten(),
            enum_value,
        })
    }

    /// The class whose method `owner` is, through nested functions.
    fn enclosing_class(&self, file: usize, owner: Option<usize>) -> Option<SymbolRef> {
        let symbols = &self.index.files()[file].symbols;
        let mut visited = BTreeSet::new();
        let mut current = owner?;
        while visited.insert(current) {
            let parent = symbols[current].parent? as usize;
            if is_type(symbols[parent].kind) {
                return Some((file, parent));
            }
            current = parent;
        }
        None
    }
}

fn is_type(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class
            | SymbolKind::Struct
            | SymbolKind::Interface
            | SymbolKind::Enum
            | SymbolKind::Object
            | SymbolKind::Trait
    )
}

fn is_callable(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Method)
}

fn same_step(left: &Step, right: &Step) -> bool {
    match (left, right) {
        (Step::Value(left), Step::Value(right)) => left == right,
        (Step::Types(left), Step::Types(right)) => left == right,
        _ => false,
    }
}

/// Candidates in `file` when it declares one, else all of them.
fn prefer_file(candidates: Vec<SymbolRef>, file: usize) -> Vec<SymbolRef> {
    let local = candidates
        .iter()
        .copied()
        .filter(|(candidate, _)| *candidate == file)
        .collect::<Vec<_>>();
    if local.is_empty() {
        candidates
    } else {
        local
    }
}

/// One link of a member chain: a name, and whether it is called.
#[derive(Debug, PartialEq, Eq)]
struct Segment<'a> {
    name: &'a str,
    call: bool,
}

/// `a.b(x, y).c` -> `a`, `b()`, `c`. A chain with any other shape (an
/// index, an operator, a literal) has no segments.
fn chain_segments(expression: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let mut rest = expression.trim();
    loop {
        let name_end = rest
            .find(|character: char| {
                !(character.is_alphanumeric() || character == '_' || character == '$')
            })
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        if name.is_empty() || name.starts_with(|character: char| character.is_ascii_digit()) {
            return Vec::new();
        }
        rest = rest[name_end..].trim_start();
        let call = rest.starts_with('(');
        if call {
            let Some(close) = crate::util::closing_paren(rest) else {
                return Vec::new();
            };
            rest = rest[close + 1..].trim_start();
        }
        segments.push(Segment { name, call });
        rest = rest.trim_start_matches(['!', '?']);
        if rest.is_empty() {
            return segments;
        }
        rest = match rest.strip_prefix('.').or_else(|| rest.strip_prefix("::")) {
            Some(next) => next.trim_start(),
            None => return Vec::new(),
        };
    }
}

#[cfg(test)]
#[path = "type_facts_tests.rs"]
mod tests;
