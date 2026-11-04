//! HTTP route handlers a file declares. Findings-free.
//!
//! - a decorator or annotation that binds a function to a route: Flask,
//!   FastAPI, and similar `@app.get("/path")`, Django REST `@api_view`,
//!   NestJS `@Get()` in a `@Controller` class, Spring `@GetMapping` and
//!   `@RequestMapping`, JAX-RS `@GET`, ASP.NET `[HttpGet]` and `[Route]`,
//!   actix and Rocket `#[get("/path")]`;
//! - a call that binds a path to a handler: Express, Fastify, Koa, and Hono
//!   `app.get("/path", handler)`, ASP.NET `app.MapGet("/path", handler)`, Go
//!   `mux.HandleFunc("/path", handler)` and router verbs, Ktor
//!   `get("/path") { }`, axum `.route("/path", get(handler))`, Django
//!   `path("route/", view)` in `urls.py`;
//! - a Next.js route module's exported `GET`/`POST` function.
//!
//! Each recognizer first checks the source text for its trigger, so a file
//! without one costs no tree walk.

use auditor_ast::AstNode;

use crate::ast_scan::nodes_of_kind;
use crate::entry_points::{EntryKind, LocalEntry};
use crate::js_api_identity::{js_arguments, js_string};
use crate::python_api_identity::positional_arguments;
use crate::{AuditedFile, Language};

const METHODS: [&str; 8] = [
    "GET",
    "POST",
    "PUT",
    "DELETE",
    "PATCH",
    "HEAD",
    "OPTIONS",
    "WEBSOCKET",
];

const STRING_KINDS: [&str; 6] = [
    "string",
    "string_literal",
    "interpreted_string_literal",
    "raw_string_literal",
    "verbatim_string_literal",
    "template_string",
];

pub(crate) fn http_entries(file: &AuditedFile, root: AstNode<'_>) -> Vec<LocalEntry> {
    let text = root.text();
    match file.language {
        Language::Python => python(file, root, text),
        Language::JavaScript | Language::TypeScript => javascript(file, root, text),
        Language::Java
            if has_any(
                text,
                &["Mapping", "@GET", "@POST", "@PUT", "@DELETE", "@PATCH"],
            ) =>
        {
            nodes_of_kind(root, &["method_declaration"])
                .into_iter()
                .filter_map(|method| annotated_route(method, &["modifiers"], ROUTE_ANNOTATIONS))
                .collect()
        }
        Language::Kotlin => kotlin(root, text),
        Language::CSharp => csharp(root, text),
        Language::Go if has_any(text, &["(\"/", "(`/", "(\"GET ", "(\"POST "]) => go(root),
        Language::Rust if text.contains("(\"/") => rust(root),
        _ => Vec::new(),
    }
}

/// Spring, JAX-RS, and Ktor-free Kotlin route annotations.
const ROUTE_ANNOTATIONS: &[&str] = &[
    "GetMapping",
    "PostMapping",
    "PutMapping",
    "DeleteMapping",
    "PatchMapping",
    "RequestMapping",
    "GET",
    "POST",
    "PUT",
    "DELETE",
    "PATCH",
    "HEAD",
    "OPTIONS",
];

// ---------------------------------------------------------------- Python

const PY_VERBS: &[&str] = &[
    "route",
    "api_route",
    "get",
    "post",
    "put",
    "delete",
    "patch",
    "head",
    "options",
    "websocket",
];

fn python(file: &AuditedFile, root: AstNode<'_>, text: &str) -> Vec<LocalEntry> {
    let mut entries = Vec::new();
    let decorated = text.contains('@')
        && (text.contains("api_view")
            || PY_VERBS
                .iter()
                .any(|verb| text.contains(&format!(".{verb}("))));
    if decorated {
        entries.extend(decorated_routes(root));
    }
    let is_urls = file.relative.file_name().and_then(|name| name.to_str()) == Some("urls.py");
    if is_urls && text.contains("path(") {
        entries.extend(django_urls(root));
    }
    entries
}

/// Functions decorated with a route ([`python_route`]).
fn decorated_routes(root: AstNode<'_>) -> Vec<LocalEntry> {
    nodes_of_kind(root, &["decorated_definition"])
        .into_iter()
        .filter_map(|decorated| {
            let definition = decorated
                .child_by_field_name("definition")
                .filter(|definition| definition.kind() == "function_definition")?;
            let detail = decorated
                .children()
                .filter(|child| child.kind() == "decorator")
                .filter_map(|decorator| decorator.named_child(0))
                .filter(|expression| expression.kind() == "call")
                .find_map(python_route)?;
            Some(LocalEntry::spanning(
                definition,
                EntryKind::HttpHandler,
                detail,
            ))
        })
        .collect()
}

/// `path("route/", view)` in `urls.py`: a route whether or not the view
/// resolves; `admin.site.urls` mounts another URL configuration.
fn django_urls(root: AstNode<'_>) -> Vec<LocalEntry> {
    nodes_of_kind(root, &["call"])
        .into_iter()
        .filter_map(|call| {
            let named = call
                .child_by_field_name("function")
                .is_some_and(|function| matches!(function.text(), "path" | "re_path" | "url"));
            let arguments = positional_arguments(call);
            let (true, [route, view, ..]) = (named, arguments.as_slice()) else {
                return None;
            };
            let route = string_value(*route)?;
            (matches!(view.kind(), "identifier" | "attribute") && !view.text().ends_with(".urls"))
                .then(|| {
                    LocalEntry::naming(
                        call,
                        *view,
                        false,
                        EntryKind::HttpHandler,
                        format!("ANY {route}"),
                    )
                })
        })
        .collect()
}

/// `@app.get("/path")`, `@bp.route("/path")`, `@api_view([..])`.
fn python_route(call: AstNode<'_>) -> Option<String> {
    let function = call.child_by_field_name("function")?;
    let verb = match function.kind() {
        "attribute" => function.child_by_field_name("attribute")?.text(),
        "identifier" if function.text() == "api_view" => return Some("ANY".to_string()),
        _ => return None,
    };
    if !PY_VERBS.contains(&verb) {
        return None;
    }
    let path = string_value(*positional_arguments(call).first()?)?;
    Some(route_detail(verb, Some(&path)))
}

// ----------------------------------------------------------- JS and TS

const JS_VERBS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "all", "options", "head",
];
const NEST_VERBS: &[&str] = &[
    "Get", "Post", "Put", "Delete", "Patch", "All", "Options", "Head",
];

fn javascript(file: &AuditedFile, root: AstNode<'_>, text: &str) -> Vec<LocalEntry> {
    let mut entries = Vec::new();
    if has_any(text, &["('/", "(\"/", "(`/", "('*'", "(\"*\""]) {
        entries.extend(
            nodes_of_kind(root, &["call_expression"])
                .into_iter()
                .filter_map(js_registration),
        );
    }
    if text.contains("@Controller") {
        for body in nodes_of_kind(root, &["class_body"]) {
            if body.parent().is_some_and(is_nest_controller) {
                entries.extend(nest_handlers(body));
            }
        }
    }
    let route_module = file
        .relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == "route");
    if route_module {
        for function in nodes_of_kind(root, &["function_declaration"]) {
            let exported = function
                .parent()
                .is_some_and(|parent| parent.kind() == "export_statement");
            let name = function
                .child_by_field_name("name")
                .map(|name| name.text())
                .filter(|name| METHODS.contains(name));
            if let (true, Some(name)) = (exported, name) {
                entries.push(LocalEntry::spanning(
                    function,
                    EntryKind::HttpHandler,
                    format!("{name} (route module)"),
                ));
            }
        }
    }
    entries
}

/// `app.get('/path', .., handler)`: a router method given a path and a
/// handler, the handler last. A named handler must resolve, so a client call
/// such as `api.get('/users', config)` is not taken for a route.
fn js_registration(call: AstNode<'_>) -> Option<LocalEntry> {
    let callee = call
        .child_by_field_name("function")
        .filter(|callee| callee.kind() == "member_expression")?;
    let verb = callee.child_by_field_name("property")?.text();
    if !JS_VERBS.contains(&verb) {
        return None;
    }
    let arguments = js_arguments(call);
    let [path, .., handler] = arguments.as_slice() else {
        return None;
    };
    let path = js_string(*path).filter(|path| is_route_path(path))?;
    let detail = route_detail(verb, Some(&path));
    match handler.kind() {
        "arrow_function" | "function_expression" | "function" => Some(LocalEntry::spanning(
            *handler,
            EntryKind::HttpHandler,
            detail,
        )),
        "identifier" | "member_expression" => Some(LocalEntry::naming(
            call,
            *handler,
            true,
            EntryKind::HttpHandler,
            detail,
        )),
        _ => None,
    }
}

/// A class declaration decorated `@Controller(..)`, directly or on its
/// `export` statement.
fn is_nest_controller(class: AstNode<'_>) -> bool {
    let decorated = |node: AstNode<'_>| {
        node.children()
            .any(|child| child.kind() == "decorator" && child.text().starts_with("@Controller"))
    };
    decorated(class) || class.parent().is_some_and(decorated)
}

/// Methods of a controller class body decorated `@Get(..)`, `@Post(..)`.
fn nest_handlers(body: AstNode<'_>) -> Vec<LocalEntry> {
    let mut entries = Vec::new();
    let mut pending: Vec<AstNode<'_>> = Vec::new();
    for member in body.named_children() {
        if member.kind() == "decorator" {
            pending.push(member);
            continue;
        }
        if member.kind() == "method_definition" {
            let decorators = pending.iter().copied().chain(
                member
                    .children()
                    .filter(|child| child.kind() == "decorator"),
            );
            if let Some(detail) = decorators.filter_map(nest_route).next() {
                entries.push(LocalEntry::spanning(member, EntryKind::HttpHandler, detail));
            }
        }
        pending.clear();
    }
    entries
}

fn nest_route(decorator: AstNode<'_>) -> Option<String> {
    let call = decorator
        .named_child(0)
        .filter(|call| call.kind() == "call_expression")?;
    let verb = call.child_by_field_name("function")?.text();
    if !NEST_VERBS.contains(&verb) {
        return None;
    }
    let path = js_arguments(call).first().and_then(|path| js_string(*path));
    Some(route_detail(verb, path.as_deref()))
}

// ---------------------------------------------- Java and Kotlin, C#

/// A declaration annotated with one of `routes` (`@GetMapping("/a")`,
/// `[HttpGet("a")]`), looked up in its `containers` children.
fn annotated_route(
    declaration: AstNode<'_>,
    containers: &[&str],
    routes: &[&str],
) -> Option<LocalEntry> {
    let annotations = declaration
        .named_children()
        .filter(|child| containers.contains(&child.kind()))
        .flat_map(|container| {
            let nested = container
                .named_children()
                .filter(|child| {
                    matches!(
                        child.kind(),
                        "annotation" | "marker_annotation" | "attribute"
                    )
                })
                .collect::<Vec<_>>();
            if nested.is_empty() && container.kind() == "annotation" {
                vec![container]
            } else {
                nested
            }
        })
        .collect::<Vec<_>>();
    let name_of = |annotation: AstNode<'_>| {
        let text = annotation.text().trim_start_matches('@');
        let end = text
            .find(|character: char| {
                !(character.is_alphanumeric() || matches!(character, '_' | '.'))
            })
            .unwrap_or(text.len());
        let name = text[..end].rsplit('.').next().unwrap_or_default();
        name.strip_suffix("Attribute").unwrap_or(name).to_string()
    };
    let route = annotations
        .iter()
        .find(|annotation| routes.contains(&name_of(**annotation).as_str()))?;
    let path = first_string(*route).or_else(|| {
        annotations
            .iter()
            .find(|annotation| name_of(**annotation) == "Path")
            .and_then(|annotation| first_string(*annotation))
    });
    Some(LocalEntry::spanning(
        declaration,
        EntryKind::HttpHandler,
        route_detail(&name_of(*route), path.as_deref()),
    ))
}

const KTOR_VERBS: &[&str] = &["get", "post", "put", "delete", "patch", "head", "options"];

fn kotlin(root: AstNode<'_>, text: &str) -> Vec<LocalEntry> {
    let mut entries = Vec::new();
    if has_any(
        text,
        &["Mapping", "@GET", "@POST", "@PUT", "@DELETE", "@PATCH"],
    ) {
        entries.extend(
            nodes_of_kind(root, &["function_declaration"])
                .into_iter()
                .filter_map(|function| {
                    annotated_route(function, &["modifiers"], ROUTE_ANNOTATIONS)
                }),
        );
    }
    if text.contains("rout") && text.contains("(\"/") {
        for call in nodes_of_kind(root, &["call_expression"]) {
            let Some(verb) = call
                .named_child(0)
                .filter(|callee| callee.kind() == "simple_identifier")
                .map(|callee| callee.text())
                .filter(|verb| KTOR_VERBS.contains(verb))
            else {
                continue;
            };
            let Some(suffix) = call
                .named_children()
                .find(|child| child.kind() == "call_suffix")
            else {
                continue;
            };
            let path = suffix
                .named_children()
                .find(|child| child.kind() == "value_arguments")
                .and_then(first_string)
                .filter(|path| is_route_path(path));
            let lambda = suffix
                .named_children()
                .find(|child| child.kind() == "annotated_lambda");
            if let (Some(path), Some(lambda)) = (path, lambda) {
                entries.push(LocalEntry::spanning(
                    lambda,
                    EntryKind::HttpHandler,
                    route_detail(verb, Some(&path)),
                ));
            }
        }
    }
    entries
}

const CSHARP_ROUTES: &[&str] = &[
    "HttpGet",
    "HttpPost",
    "HttpPut",
    "HttpDelete",
    "HttpPatch",
    "HttpHead",
    "HttpOptions",
    "Route",
];
const CSHARP_MAPS: &[&str] = &["MapGet", "MapPost", "MapPut", "MapDelete", "MapPatch"];

fn csharp(root: AstNode<'_>, text: &str) -> Vec<LocalEntry> {
    let mut entries = Vec::new();
    if has_any(text, &["[Http", "[Route", ", Http", ", Route"]) {
        entries.extend(
            nodes_of_kind(root, &["method_declaration"])
                .into_iter()
                .filter_map(|method| annotated_route(method, &["attribute_list"], CSHARP_ROUTES)),
        );
    }
    if text.contains(".Map") {
        let callee = ("member_access_expression", "name");
        for found in registrations(root, "invocation_expression", callee, CSHARP_MAPS) {
            if is_route_path(&found.path) {
                let detail = route_detail(found.verb, Some(&found.path));
                entries.extend(handler_entry(
                    found.call,
                    found.handler,
                    &CSHARP_HANDLERS,
                    false,
                    detail,
                ));
            }
        }
    }
    entries
}

// -------------------------------------------------------------------- Go

/// Router methods: net/http and gorilla `HandleFunc`/`Handle`, chi and
/// fiber `Get`, gin and echo `GET`.
const GO_VERBS: &[&str] = &[
    "HandleFunc",
    "Handle",
    "Get",
    "Post",
    "Put",
    "Delete",
    "Patch",
    "Head",
    "Options",
    "GET",
    "POST",
    "PUT",
    "DELETE",
    "PATCH",
    "HEAD",
    "OPTIONS",
    "Any",
];

fn go(root: AstNode<'_>) -> Vec<LocalEntry> {
    let mut entries = Vec::new();
    let callee = ("selector_expression", "field");
    for found in registrations(root, "call_expression", callee, GO_VERBS) {
        let (verb, pattern) = (found.verb, &found.path);
        // Go 1.22 patterns carry their method: `"GET /users/{id}"`.
        let detail = match pattern.split_once(' ') {
            Some((method, path)) if METHODS.contains(&method) && is_route_path(path) => {
                pattern.clone()
            }
            _ if is_route_path(pattern) => route_detail(verb, Some(pattern)),
            _ => continue,
        };
        // A mixed-case verb (`Get`) is also a cache or map method; its named
        // handler must resolve. `HandleFunc` and upper-case verbs are routers'.
        let required = verb.chars().nth(1).is_some_and(char::is_lowercase)
            && !matches!(verb, "HandleFunc" | "Handle");
        entries.extend(handler_entry(
            found.call,
            found.handler,
            &GO_HANDLERS,
            required,
            detail,
        ));
    }
    entries
}

// ------------------------------------------------------------------ Rust

const RUST_VERBS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "head", "options", "route",
];
const AXUM_VERBS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "head", "options", "any",
];

fn rust(root: AstNode<'_>) -> Vec<LocalEntry> {
    let mut entries = Vec::new();
    let containers = std::iter::once(root).chain(nodes_of_kind(root, &["declaration_list"]));
    for container in containers {
        let mut attributes: Vec<AstNode<'_>> = Vec::new();
        for item in container.named_children() {
            match item.kind() {
                "attribute_item" => attributes.push(item),
                "function_item" => {
                    if let Some(detail) = attributes
                        .iter()
                        .find_map(|attribute| rust_route(*attribute))
                    {
                        entries.push(LocalEntry::spanning(item, EntryKind::HttpHandler, detail));
                    }
                    attributes.clear();
                }
                _ => attributes.clear(),
            }
        }
    }
    if root.text().contains(".route(") {
        for call in nodes_of_kind(root, &["call_expression"]) {
            entries.extend(axum_route(call));
        }
    }
    entries
}

/// `#[get("/users")]`, `#[actix_web::post("/")]`, `#[route("/", method = ..)]`.
fn rust_route(attribute: AstNode<'_>) -> Option<String> {
    let text = attribute.text().trim_start_matches("#[");
    let (path, _) = text.split_once('(')?;
    let verb = path.rsplit("::").next()?.trim();
    if !RUST_VERBS.contains(&verb) {
        return None;
    }
    let route = first_string(attribute).filter(|route| is_route_path(route))?;
    Some(route_detail(verb, Some(&route)))
}

/// axum `.route("/users", get(list).post(create))`: each method router's
/// handler.
fn axum_route(call: AstNode<'_>) -> Vec<LocalEntry> {
    let is_route = call
        .child_by_field_name("function")
        .filter(|function| function.kind() == "field_expression")
        .and_then(|function| function.child_by_field_name("field"))
        .is_some_and(|field| field.text() == "route");
    let arguments = call_arguments(call);
    let (true, [path, methods]) = (is_route, arguments.as_slice()) else {
        return Vec::new();
    };
    let Some(path) = string_value(*path).filter(|path| is_route_path(path)) else {
        return Vec::new();
    };
    let mut routers = nodes_of_kind(*methods, &["call_expression"]);
    if methods.kind() == "call_expression" {
        routers.push(*methods);
    }
    routers
        .into_iter()
        .filter_map(|router| {
            let callee = router.child_by_field_name("function")?.text();
            let verb = callee.rsplit(['.', ':']).next()?;
            if !AXUM_VERBS.contains(&verb) {
                return None;
            }
            let handler = router.child_by_field_name("arguments")?.named_child(0)?;
            matches!(handler.kind(), "identifier" | "scoped_identifier").then(|| {
                LocalEntry::naming(
                    router,
                    handler,
                    false,
                    EntryKind::HttpHandler,
                    route_detail(verb, Some(&path)),
                )
            })
        })
        .collect()
}

// --------------------------------------------------------------- helpers

/// The handler shapes a registration accepts: inline functions, entered by
/// their span, and references, entered by the name they resolve to.
struct HandlerShapes {
    inline: &'static [&'static str],
    named: &'static [&'static str],
}

const CSHARP_HANDLERS: HandlerShapes = HandlerShapes {
    inline: &["lambda_expression", "anonymous_method_expression"],
    named: &["identifier", "member_access_expression"],
};

const GO_HANDLERS: HandlerShapes = HandlerShapes {
    inline: &["func_literal"],
    named: &["identifier", "selector_expression"],
};

fn handler_entry(
    site: AstNode<'_>,
    handler: AstNode<'_>,
    shapes: &HandlerShapes,
    required: bool,
    detail: String,
) -> Option<LocalEntry> {
    if shapes.inline.contains(&handler.kind()) {
        Some(LocalEntry::spanning(
            handler,
            EntryKind::HttpHandler,
            detail,
        ))
    } else if shapes.named.contains(&handler.kind()) {
        Some(LocalEntry::naming(
            site,
            handler,
            required,
            EntryKind::HttpHandler,
            detail,
        ))
    } else {
        None
    }
}

/// The method name of a `receiver.name(..)` call whose callee is a `member`
/// node with the name in `field`, when `names` lists it.
fn member_call<'a>(
    call: AstNode<'a>,
    member: &str,
    field: &str,
    names: &[&str],
) -> Option<&'a str> {
    let name = call
        .child_by_field_name("function")
        .filter(|function| function.kind() == member)?
        .child_by_field_name(field)?
        .text();
    names.contains(&name).then_some(name)
}

/// A call binding a path to a handler: `receiver.verb(path, .., handler)`.
struct Registration<'a> {
    call: AstNode<'a>,
    verb: &'a str,
    path: String,
    handler: AstNode<'a>,
}

/// The `call_kind` calls whose `(member kind, name field)` callee names one
/// of `verbs`, with a string first argument and a handler last.
fn registrations<'a>(
    root: AstNode<'a>,
    call_kind: &str,
    (member, field): (&str, &str),
    verbs: &[&str],
) -> Vec<Registration<'a>> {
    nodes_of_kind(root, &[call_kind])
        .into_iter()
        .filter_map(|call| {
            let verb = member_call(call, member, field, verbs)?;
            let arguments = call_arguments(call);
            let [path, .., handler] = arguments.as_slice() else {
                return None;
            };
            Some(Registration {
                call,
                verb,
                path: string_value(*path)?,
                handler: *handler,
            })
        })
        .collect()
}

/// A call's argument expressions, without comments or C#'s `argument`
/// wrappers.
fn call_arguments(call: AstNode<'_>) -> Vec<AstNode<'_>> {
    call.child_by_field_name("arguments")
        .map(|list| {
            list.named_children()
                .filter(|argument| !argument.kind().ends_with("comment"))
                .filter_map(|argument| {
                    if argument.kind() == "argument" {
                        argument.named_child(0)
                    } else {
                        Some(argument)
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `GET /users` from a verb (`get`, `GetMapping`, `HttpGet`, `MapGet`) and a
/// path; `ANY` for a verb naming no one method (`route`, `HandleFunc`).
fn route_detail(verb: &str, path: Option<&str>) -> String {
    let stem = verb
        .strip_prefix("Http")
        .or_else(|| verb.strip_prefix("Map"))
        .unwrap_or(verb);
    let stem = stem
        .strip_suffix("Mapping")
        .unwrap_or(stem)
        .to_ascii_uppercase();
    let method = if METHODS.contains(&stem.as_str()) {
        stem
    } else {
        "ANY".to_string()
    };
    match path {
        Some(path) => format!("{method} {path}"),
        None => method,
    }
}

fn is_route_path(path: &str) -> bool {
    path.starts_with('/') || path == "*"
}

/// The text of a string literal node, without prefix, quotes, or raw-string
/// hashes (`r#".."#`, `@".."`, `f".."`).
fn string_value(node: AstNode<'_>) -> Option<String> {
    if !STRING_KINDS.contains(&node.kind()) {
        return None;
    }
    let text = node
        .text()
        .trim()
        .trim_start_matches(|character: char| "rRbBuUfF@$#".contains(character));
    let quote = ['"', '\'', '`']
        .into_iter()
        .find(|quote| text.starts_with(*quote))?;
    Some(
        text.trim_end_matches('#')
            .trim_start_matches(quote)
            .trim_end_matches(quote)
            .to_string(),
    )
}

/// The first string literal inside `node`.
fn first_string(node: AstNode<'_>) -> Option<String> {
    nodes_of_kind(node, &STRING_KINDS)
        .into_iter()
        .min_by_key(|literal| literal.start_byte())
        .and_then(string_value)
}

fn has_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

#[cfg(test)]
#[path = "entry_points_http_tests.rs"]
mod tests;
