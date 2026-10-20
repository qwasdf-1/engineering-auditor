//! Request input and security sinks for the data-flow layer
//! (`auditor_ast::FunctionFlow`). Findings-free.
//!
//! A value comes from a request when one of its origins is a request
//! member (`req.query.table`, `request.args`, `Request.Query`), a request
//! accessor call (`c.Query("t")`, `request.getParameter("t")`,
//! `chi.URLParam(r, "t")`), or a parameter bound from the request by an
//! annotation (`@RequestParam`, `@PathVariable`, `[FromQuery]`) or a
//! FastAPI marker (`= Query(..)`). Origins are followed through method calls
//! on a value (`t.trim()`, `request.args.get("t")`) and through formatting
//! and conversion calls, but not through other calls, which may validate or
//! escape (`escapeId(t)`, `ALLOWED.get(t)`).

use auditor_ast::{call_receiver, AstNode, CallFlow, FunctionFlow, Origin};

/// Member chains that hold request input.
const REQUEST_MEMBERS: &[&str] = &[
    // Express, Koa, Fastify, Next.js, AWS Lambda.
    "req.query",
    "req.params",
    "req.body",
    "req.headers",
    "req.cookies",
    "request.query",
    "request.params",
    "request.body",
    "request.headers",
    "ctx.query",
    "ctx.params",
    "ctx.request.query",
    "ctx.request.body",
    "event.queryStringParameters",
    "event.pathParameters",
    "event.body",
    // Flask, Django, Starlette, aiohttp.
    "request.args",
    "request.form",
    "request.values",
    "request.json",
    "request.data",
    "request.files",
    "request.cookies",
    "request.GET",
    "request.POST",
    "request.query_params",
    "request.path_params",
    "request.match_info",
    // ASP.NET.
    "Request.Query",
    "Request.Form",
    "Request.RouteValues",
    "Request.Headers",
    "Request.Cookies",
    // net/http.
    "r.URL.Query",
    "r.Form",
    "r.PostForm",
];

/// Calls that return request input, by callee.
const REQUEST_CALLS: &[&str] = &[
    // gin, echo, fiber.
    "c.Query",
    "c.DefaultQuery",
    "c.GetQuery",
    "c.Param",
    "c.PostForm",
    "c.DefaultPostForm",
    "c.QueryParam",
    "c.FormValue",
    "c.Params",
    "ctx.Query",
    "ctx.Params",
    // net/http, chi, gorilla/mux.
    "r.FormValue",
    "r.PostFormValue",
    "r.PathValue",
    "chi.URLParam",
    "mux.Vars",
    // hono.
    "c.req.query",
    "c.req.param",
    "c.req.json",
    "c.req.header",
];

/// Servlet accessors that return request input, on a request receiver.
const SERVLET_ACCESSORS: &[&str] = &[
    "getParameter",
    "getParameterValues",
    "getParameterMap",
    "getQueryString",
    "getHeader",
    "getHeaders",
    "getPathInfo",
    "getRequestURI",
    "getCookies",
];

/// Parameter annotations and markers that bind request input.
const REQUEST_PARAMETER_MARKERS: &[&str] = &[
    "@RequestParam",
    "@PathVariable",
    "@RequestBody",
    "@RequestHeader",
    "@CookieValue",
    "@QueryParam",
    "@PathParam",
    "@FormParam",
    "@HeaderParam",
    "[FromQuery",
    "[FromRoute",
    "[FromBody",
    "[FromForm",
    "[FromHeader",
    "Query(",
    "Path(",
    "Body(",
    "Form(",
    "Header(",
    "Cookie(",
];

/// Calls whose result is made from their arguments: formatting, joining,
/// and conversion to text.
const FORMATTING_CALLS: &[&str] = &[
    "format",
    "formatted",
    "Sprintf",
    "Sprint",
    "join",
    "Join",
    "concat",
    "Concat",
    "valueOf",
    "String",
    "str",
    "toString",
    "ToString",
];

/// Whether `expression`'s value may come from request input.
pub(crate) fn from_request(expression: AstNode<'_>) -> bool {
    let flow = FunctionFlow::enclosing(expression);
    flow.origins(expression, &request_policy)
        .iter()
        .any(is_request_origin)
}

/// Method calls pass their receiver on; formatting and conversion calls
/// pass everything on; other calls are opaque.
fn request_policy(call: AstNode<'_>) -> CallFlow {
    if FORMATTING_CALLS.contains(&callee_name(call)) {
        CallFlow::All
    } else if call_receiver(call).is_some() {
        CallFlow::Receiver
    } else {
        CallFlow::Opaque
    }
}

fn is_request_origin(origin: &Origin<'_>) -> bool {
    match origin {
        Origin::Member(member) => REQUEST_MEMBERS
            .iter()
            .any(|prefix| chain_starts_with(member.text(), prefix)),
        Origin::Call(call) => is_request_call(*call),
        Origin::Parameter(binding) => REQUEST_PARAMETER_MARKERS
            .iter()
            .any(|marker| binding.node.text().contains(marker)),
        _ => false,
    }
}

fn is_request_call(call: AstNode<'_>) -> bool {
    let callee = callee_text(call);
    if REQUEST_CALLS.contains(&callee) {
        return true;
    }
    let receiver = call_receiver(call).map(|receiver| receiver.text().to_ascii_lowercase());
    SERVLET_ACCESSORS.contains(&callee_name(call))
        && receiver.is_some_and(|receiver| receiver.ends_with("request") || receiver == "req")
}

/// Whether the member chain `text` is `prefix` or extends it
/// (`req.query.table`, `request.args["t"]`).
fn chain_starts_with(text: &str, prefix: &str) -> bool {
    let compact = text.split_whitespace().collect::<String>();
    compact
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[', '(']))
}

fn callee_text(call: AstNode<'_>) -> &str {
    if let (Some(object), Some(name)) = (
        call.child_by_field_name("object"),
        call.child_by_field_name("name"),
    ) {
        // A Java method invocation names its method apart from its object.
        let start = object.start_byte() - call.start_byte();
        let end = name.end_byte() - call.start_byte();
        return &call.text()[start..end];
    }
    call.child_by_field_name("function")
        .or_else(|| call.named_child(0))
        .map_or("", |callee| callee.text())
}

pub(crate) fn callee_name(call: AstNode<'_>) -> &str {
    if let Some(name) = call.child_by_field_name("name") {
        return name.text();
    }
    callee_text(call)
        .rsplit(['.', ':'])
        .next()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    /// Whether the last occurrence of `name` in `source` holds request input.
    fn requested(language: AstLang, source: &str, name: &str) -> bool {
        let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
        let mut reads = Vec::new();
        fn all<'a>(node: AstNode<'a>, text: &str, out: &mut Vec<AstNode<'a>>) {
            if node.text() == text && node.kind().contains("identifier") {
                out.push(node);
            }
            node.children().for_each(|child| all(child, text, out));
        }
        all(tree.root_node(), name, &mut reads);
        from_request(*reads.last().unwrap_or_else(|| panic!("no {name}")))
    }

    #[test]
    fn request_input_is_traced_to_its_uses() {
        let express = "app.get('/t', (req, res) => {\n  const { table } = req.query;\n  const t = table.trim();\n  db.query('SELECT * FROM ' + t);\n});\n";
        assert!(requested(AstLang::JavaScript, express, "t"));
        let escaped = "app.get('/t', (req, res) => {\n  const t = escapeId(req.query.table);\n  db.query('SELECT * FROM ' + t);\n});\n";
        assert!(!requested(AstLang::JavaScript, escaped, "t"));
        let flask = "@app.route('/t')\ndef show():\n    t = request.args.get('t')\n    cur.execute('SELECT * FROM %s' % t)\n";
        assert!(requested(AstLang::Python, flask, "t"));
        let spring = "class C { List f(@RequestParam String table) {\n  String t = table.toLowerCase();\n  return jdbc.query(\"SELECT * FROM \" + t);\n} }\n";
        assert!(requested(AstLang::Java, spring, "t"));
        let servlet = "class C { void f(HttpServletRequest request) {\n  String t = request.getParameter(\"t\");\n  st.execute(\"SELECT * FROM \" + t);\n} }\n";
        assert!(requested(AstLang::Java, servlet, "t"));
        let gin = "package p\nfunc h(c *gin.Context) {\n  t := c.Query(\"t\")\n  db.Query(fmt.Sprintf(\"SELECT * FROM %s\", t))\n}\n";
        assert!(requested(AstLang::Go, gin, "t"));
        let configured = "class C { void f(Options options) {\n  String t = options.stagingSchema;\n  h.execute(\"TRUNCATE \" + t);\n} }\n";
        assert!(!requested(AstLang::Java, configured, "t"));
    }
}
