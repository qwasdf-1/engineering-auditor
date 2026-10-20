macro_rules! halstead_token_set {
    ($kind:expr, $($token:literal)|+ $(,)?) => {{
        static TOKENS: std::sync::LazyLock<std::collections::HashSet<&'static str>> =
            std::sync::LazyLock::new(|| std::collections::HashSet::from([$($token),+]));
        TOKENS.contains($kind)
    }};
}

pub(super) fn is_halstead_operand(language: crate::AstLang, kind: &str) -> bool {
    use crate::AstLang;
    match language {
        AstLang::Python => halstead_token_set!(
            kind,
            "identifier" | "integer" | "float" | "string" | "true" | "false" | "none" | "type"
        ),
        AstLang::JavaScript => halstead_token_set!(
            kind,
            "identifier"
                | "property_identifier"
                | "shorthand_property_identifier"
                | "number"
                | "string"
                | "string_fragment"
                | "template_string"
                | "regex"
                | "true"
                | "false"
                | "null"
                | "undefined"
        ),
        AstLang::TypeScript => halstead_token_set!(
            kind,
            "identifier"
                | "property_identifier"
                | "shorthand_property_identifier"
                | "type_identifier"
                | "number"
                | "string"
                | "string_fragment"
                | "template_string"
                | "regex"
                | "true"
                | "false"
                | "null"
                | "undefined"
        ),
        AstLang::Go => halstead_token_set!(
            kind,
            "identifier"
                | "field_identifier"
                | "type_identifier"
                | "package_identifier"
                | "int_literal"
                | "float_literal"
                | "imaginary_literal"
                | "rune_literal"
                | "raw_string_literal"
                | "interpreted_string_literal"
                | "true"
                | "false"
                | "nil"
                | "iota"
        ),
        AstLang::Rust => halstead_token_set!(
            kind,
            "identifier"
                | "field_identifier"
                | "type_identifier"
                | "integer_literal"
                | "float_literal"
                | "string_literal"
                | "raw_string_literal"
                | "char_literal"
                | "boolean_literal"
                | "true"
                | "false"
        ),
        AstLang::Kotlin => halstead_token_set!(
            kind,
            "simple_identifier"
                | "type_identifier"
                | "integer_literal"
                | "long_literal"
                | "hex_literal"
                | "bin_literal"
                | "unsigned_literal"
                | "real_literal"
                | "string_content"
                | "character_literal"
                | "boolean_literal"
                | "true"
                | "false"
                | "null"
        ),
        AstLang::Java => halstead_token_set!(
            kind,
            "identifier"
                | "type_identifier"
                | "field_identifier"
                | "decimal_integer_literal"
                | "hex_integer_literal"
                | "octal_integer_literal"
                | "binary_integer_literal"
                | "decimal_floating_point_literal"
                | "string_literal"
                | "string_fragment"
                | "character_literal"
                | "true"
                | "false"
                | "null"
        ),
        AstLang::CSharp => halstead_token_set!(
            kind,
            "identifier"
                | "type_identifier"
                | "integer_literal"
                | "real_literal"
                | "string_literal"
                | "verbatim_string_literal"
                | "interpolated_string_expression"
                | "character_literal"
                | "true"
                | "false"
                | "null"
        ),
        AstLang::Julia => halstead_token_set!(
            kind,
            "identifier"
                | "integer_literal"
                | "float_literal"
                | "string_literal"
                | "character_literal"
                | "true"
                | "false"
                | "nothing"
                | "missing"
        ),
        AstLang::C => halstead_token_set!(
            kind,
            "identifier"
                | "field_identifier"
                | "type_identifier"
                | "number_literal"
                | "string_literal"
                | "char_literal"
                | "concatenated_string"
                | "true"
                | "false"
                | "null"
        ),
        AstLang::Cpp => halstead_token_set!(
            kind,
            "identifier"
                | "field_identifier"
                | "type_identifier"
                | "namespace_identifier"
                | "template_type"
                | "number_literal"
                | "string_literal"
                | "char_literal"
                | "raw_string_literal"
                | "concatenated_string"
                | "user_defined_literal"
                | "true"
                | "false"
                | "null"
                | "nullptr"
                | "this"
        ),
        AstLang::Ruby => halstead_token_set!(
            kind,
            "identifier"
                | "instance_variable"
                | "class_variable"
                | "global_variable"
                | "constant"
                | "integer"
                | "float"
                | "complex"
                | "rational"
                | "string"
                | "string_content"
                | "symbol"
                | "simple_symbol"
                | "hash_key_symbol"
                | "true"
                | "false"
                | "nil"
        ),
        AstLang::Bash => false,
    }
}
