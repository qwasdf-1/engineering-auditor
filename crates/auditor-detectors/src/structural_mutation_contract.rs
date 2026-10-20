//! The typed-language contract check for `collection_parameter_mutation`:
//! whether a parameter's declared type makes an in-place mutation visible to
//! the caller.

use super::*;

/// In a typed language a mutation surprises the caller only when the
/// parameter is declared as a collection (a `StringBuilder` or tree-builder
/// parameter is an output sink by design), the function is not private, and
/// the function does not hand the parameter to itself (a recursive walk
/// threading a `visited` set or an accumulator).
pub(super) fn mutation_is_contract_visible(
    file: &StructureFile,
    owner: u32,
    function: &StructureSymbol,
    parameter: &StructureSymbol,
) -> bool {
    const JAVA_COLLECTIONS: &[&str] = &[
        "ArrayDeque",
        "ArrayList",
        "Collection",
        "Deque",
        "HashMap",
        "HashSet",
        "LinkedHashMap",
        "LinkedHashSet",
        "LinkedList",
        "List",
        "Map",
        "NavigableMap",
        "NavigableSet",
        "PriorityQueue",
        "Queue",
        "Set",
        "SortedMap",
        "SortedSet",
        "TreeMap",
        "TreeSet",
    ];
    const KOTLIN_COLLECTIONS: &[&str] = &[
        "ArrayDeque",
        "ArrayList",
        "HashMap",
        "HashSet",
        "LinkedHashMap",
        "LinkedHashSet",
        "MutableCollection",
        "MutableList",
        "MutableMap",
        "MutableSet",
    ];
    const CSHARP_COLLECTIONS: &[&str] = &[
        "Collection",
        "Dictionary",
        "HashSet",
        "ICollection",
        "IDictionary",
        "IList",
        "ISet",
        "LinkedList",
        "List",
        "Queue",
        "SortedDictionary",
        "SortedList",
        "SortedSet",
        "Stack",
    ];
    let declaration = parameter.declaration.as_deref().unwrap_or_default();
    // `name: Type<..>` (Kotlin) or `Type<..> name` (Java, C#).
    let simple_type = |annotation: &str| {
        annotation
            .trim()
            .split(['<', '?', '['])
            .next()
            .and_then(|name| name.rsplit('.').next())
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };
    let collection = match file.language {
        Language::Java => parameter_type_name(declaration, &parameter.name)
            .is_some_and(|type_name| JAVA_COLLECTIONS.contains(&type_name)),
        Language::Kotlin => declaration.split_once(':').is_some_and(|(_, annotation)| {
            KOTLIN_COLLECTIONS.contains(&simple_type(annotation).as_str())
        }),
        // An extension method's `this` receiver is the value the method is
        // written to act on, like a method's own instance.
        Language::CSharp => {
            parameter_type_name(declaration, &parameter.name)
                .is_some_and(|type_name| CSHARP_COLLECTIONS.contains(&type_name))
                && !declaration
                    .split_whitespace()
                    .any(|word| matches!(word, "ref" | "out" | "this"))
        }
        _ => declaration
            .split_once(':')
            .map(|(_, annotation)| annotation.trim())
            .is_some_and(|annotation| {
                annotation.ends_with("[]")
                    || ["Array<", "Map<", "Set<", "WeakMap<", "WeakSet<"]
                        .iter()
                        .any(|generic| annotation.starts_with(generic))
            }),
    };
    let private = match file.language {
        Language::Java | Language::CSharp => !function.is_public,
        Language::Kotlin => function.declaration.as_deref().is_some_and(|header| {
            header
                .split_whitespace()
                .any(|word| matches!(word, "private" | "internal"))
        }),
        Language::JavaScript | Language::TypeScript => script_function_is_private(file, function),
        _ => false,
    };
    let recursive = file.calls.iter().any(|call| {
        call.owner == Some(owner)
            && call
                .callee
                .rsplit(['.', ':'])
                .next()
                .is_some_and(|callee| callee == function.name)
            && call.arguments.as_deref().is_some_and(|arguments| {
                crate::util::identifiers(arguments).contains(&parameter.name)
            })
    });
    collection && !private && !recursive
}

/// A `#private`, `private`, or `protected` method; a function nested in
/// another; or, in a TypeScript module, a top-level function it does not
/// `export` (JavaScript may export through `module.exports` instead).
fn script_function_is_private(file: &StructureFile, function: &StructureSymbol) -> bool {
    let head = function
        .declaration
        .as_deref()
        .unwrap_or_default()
        .trim_start();
    let words = crate::util::identifiers(head);
    if function.name.starts_with('#')
        || head.starts_with('#')
        || words
            .iter()
            .any(|word| matches!(word.as_str(), "private" | "protected"))
    {
        return true;
    }
    // Enclosing symbols, past the `const` an arrow function is bound to.
    let mut enclosing = std::iter::successors(function.parent, |&parent| {
        file.symbols
            .get(parent as usize)
            .and_then(|symbol| symbol.parent)
    })
    .filter_map(|parent| file.symbols.get(parent as usize))
    .map(|symbol| symbol.kind)
    .filter(|kind| {
        matches!(
            kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Class
        )
    });
    match enclosing.next() {
        Some(SymbolKind::Function | SymbolKind::Method) => true,
        Some(_) => false,
        None => file.language == Language::TypeScript && !words.iter().any(|word| word == "export"),
    }
}
