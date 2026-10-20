//! The reads of a value bound to a local, from the data-flow layer
//! (`auditor_ast::FunctionFlow`), for owners that follow a handle to its
//! uses. Findings-free.

use auditor_ast::{AstNode, FunctionFlow};

/// The reads the binding named by `target` may reach: later in its
/// function or in the anonymous functions within it, and earlier in a loop
/// on the next iteration, but not after a reassignment replaces it. Empty
/// when the layer reads no binding at `target`.
pub(crate) fn reads_of_target(target: AstNode<'_>) -> Vec<AstNode<'_>> {
    let flow = FunctionFlow::enclosing(target);
    flow.bindings()
        .iter()
        .find(|binding| binding.target == target)
        .map(|binding| flow.reads_of(binding))
        .unwrap_or_default()
}

/// Whether the handle bound to `target` is released or handed on: a read
/// of it either is not the receiver of a method call (`method_on` gives
/// none, so ownership moves) or calls one of the `releasing` methods.
pub(crate) fn released<'a>(
    target: AstNode<'a>,
    method_on: impl Fn(AstNode<'a>) -> Option<&'a str>,
    releasing: &[&str],
) -> bool {
    reads_of_target(target)
        .into_iter()
        .any(|read| method_on(read).is_none_or(|method| releasing.contains(&method)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use auditor_ast::{AstLang, AstParser};

    /// The lines of the reads of the binding named by the first `name`.
    fn read_lines(language: AstLang, source: &str, name: &str) -> Vec<usize> {
        let tree = AstParser::new().unwrap().parse(&language, source).unwrap();
        fn first<'a>(node: AstNode<'a>, name: &str) -> Option<AstNode<'a>> {
            if node.kind().contains("identifier") && node.text() == name {
                return Some(node);
            }
            node.children().find_map(|child| first(child, name))
        }
        reads_of_target(first(tree.root_node(), name).unwrap())
            .into_iter()
            .map(|read| read.start_position().0 + 1)
            .collect()
    }

    #[test]
    fn handles_are_followed_to_their_reads_in_each_language() {
        let c = "void f(void) {\n  FILE *fp = fopen(\"a\", \"r\");\n  fread(b, 1, 1, fp);\n  fclose(fp);\n}\n";
        assert_eq!(read_lines(AstLang::C, c, "fp"), [3, 4]);
        let go = "package p\nfunc f() {\n\tfile, err := os.Open(\"a\")\n\t_ = err\n\tdefer func() { file.Close() }()\n}\n";
        assert_eq!(read_lines(AstLang::Go, go, "file"), [5]);
        let java = "class A { void f() throws Exception {\n  InputStream in = new FileInputStream(\"a\");\n  in.read();\n  in.close();\n} }\n";
        assert_eq!(read_lines(AstLang::Java, java, "in"), [3, 4]);
        let csharp = "class A { void F() {\n  var s = File.OpenRead(\"a\");\n  s.Dispose();\n} }\n";
        assert_eq!(read_lines(AstLang::CSharp, csharp, "s"), [3]);
        let kotlin = "fun f() {\n    val input = FileInputStream(\"a\")\n    input.close()\n}\n";
        assert_eq!(read_lines(AstLang::Kotlin, kotlin, "input"), [3]);
        let js = "function f() {\n  const fd = fs.openSync('a');\n  fs.closeSync(fd);\n}\n";
        assert_eq!(read_lines(AstLang::JavaScript, js, "fd"), [3]);
        let python = "def f():\n    h = open('a')\n    h.close()\n";
        assert_eq!(read_lines(AstLang::Python, python, "h"), [3]);
    }

    #[test]
    fn a_reassignment_ends_the_first_handles_reads() {
        let python = "def f():\n    h = open('a')\n    h = open('b')\n    h.close()\n";
        assert!(read_lines(AstLang::Python, python, "h").is_empty());
    }
}
