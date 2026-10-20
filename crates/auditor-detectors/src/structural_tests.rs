use super::*;

#[test]
fn rust_dyn_and_string_fixtures_are_not_escape_hatch_annotations() {
    let lines = [
        "fn render(value: &dyn Display) -> String {",
        "    let fixture = r#\"def parse(value: Any) -> Any: pass\"#;",
        "    value.to_string()",
        "}",
    ];
    assert!(
        escape_hatch_type_density_per_file("src/render.rs", &Language::Rust, &lines).is_empty()
    );
}

#[test]
fn typescript_generic_placeholder_anys_are_not_escape_hatches() {
    let mut depth = 0;
    let placeholders = [
        "export type AnyMap = Map<any, any>",
        "<I extends Input = {}, R extends HandlerResponse<any> = any>(",
        "function handlers<",
        "  R extends HandlerResponse<any> = any,",
        ">() {}",
        "  TQueriesOptions extends any[],",
        "type Options = UseQueryOptions<",
        "    any,",
        "    any",
        ">;",
    ];
    for line in placeholders {
        assert_eq!(typescript_escape_anys(line, &mut depth), 0, "{line}");
    }
    assert_eq!(typescript_escape_anys("function f(a: any, b: any): any {", &mut depth), 3);
    assert_eq!(typescript_escape_anys("const ok = a < b ? (x as any) : y;", &mut depth), 1);
    assert_eq!(typescript_escape_anys("const f = (x: any) => x as any;", &mut depth), 2);
}
