//! Parameter lists another declaration fixes, which `too_many_parameters`
//! does not measure: foreign bindings, overrides, and Mixin injectors.

use super::*;

/// Mixin injector annotations (SpongePowered Mixin, MixinExtras): the
/// callback takes its target method's parameters, plus captured locals and
/// the callback handle.
const MIXIN_INJECTORS: &[&str] = &[
    "Inject",
    "Redirect",
    "ModifyArg",
    "ModifyArgs",
    "ModifyConstant",
    "ModifyVariable",
    "ModifyExpressionValue",
    "ModifyReceiver",
    "ModifyReturnValue",
    "WrapOperation",
    "WrapWithCondition",
    "WrapMethod",
];

/// A signature another declaration fixes. An override (Java `@Override`;
/// Kotlin, C#, and TypeScript `override`) restates its supertype's method,
/// which carries the finding where the repository declares it; a Mixin
/// injector callback must mirror the method it injects into. A javax or
/// Guice `@Inject` constructor is not a Mixin callback: the file must import
/// the Mixin library.
pub(super) fn mirrors_another_signature(file: &StructureFile, function: &StructureSymbol) -> bool {
    let Some(declaration) = function.declaration.as_deref() else {
        return false;
    };
    let mut words = declaration.split_whitespace();
    match file.language {
        Language::Java => words.any(|word| {
            let Some(annotation) = word.strip_prefix('@') else {
                return false;
            };
            let name = annotation.rsplit('.').next().unwrap_or(annotation);
            name == "Override" || MIXIN_INJECTORS.contains(&name) && imports_mixin(file)
        }),
        Language::Kotlin | Language::CSharp | Language::TypeScript => {
            words.any(|word| word == "override")
        }
        _ => false,
    }
}

fn imports_mixin(file: &StructureFile) -> bool {
    file.imports.iter().any(|import| {
        import
            .module
            .as_deref()
            .or(import.canonical.as_deref())
            .is_some_and(|module| {
                module.starts_with("org.spongepowered.asm.mixin")
                    || module.starts_with("com.llamalad7.mixinextras")
            })
    })
}

pub(super) fn is_foreign_binding(file: &StructureFile, function: &StructureSymbol) -> bool {
    let keyword = match file.language {
        Language::Kotlin => "external",
        Language::CSharp => "extern",
        _ => return false,
    };
    function
        .declaration
        .as_deref()
        .is_some_and(|declaration| declaration.split_whitespace().any(|word| word == keyword))
}
