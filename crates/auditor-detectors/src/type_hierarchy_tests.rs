use super::BaseTarget;
use crate::structure_index::{index_sources, StructureIndex};
use crate::Language;

fn class(index: &StructureIndex, path: &str, name: &str) -> (usize, usize) {
    let file = index
        .files()
        .iter()
        .position(|file| file.path == path)
        .unwrap_or_else(|| panic!("no file {path}"));
    let symbol = index.files()[file]
        .symbols
        .iter()
        .position(|symbol| symbol.name == name && super::is_type(symbol.kind))
        .unwrap_or_else(|| panic!("no type {name} in {path}"));
    (file, symbol)
}

fn target(index: &StructureIndex, path: &str, name: &str) -> BaseTarget {
    index
        .hierarchy()
        .base(class(index, path, name))
        .map(|base| base.target.clone())
        .unwrap_or_else(|| panic!("{name} has no base"))
}

#[test]
fn python_bases_resolve_across_files_to_an_external_root() {
    let index = index_sources(
        "hierarchy-python",
        &[
            (
                "app/util/process.py",
                Language::Python,
                "import multiprocessing as mp\n\n\nclass BaseProcess(mp.Process):\n    pass\n\n\nclass AppProcess(BaseProcess):\n    pass\n",
            ),
            (
                "app/embeddings.py",
                Language::Python,
                "from app.util.process import AppProcess\n\n\nclass EmbeddingProcess(AppProcess):\n    def run(self):\n        pass\n",
            ),
            (
                "app/worker.py",
                Language::Python,
                "from multiprocessing import Process as P\n\n\nclass Worker(P):\n    pass\n\n\nclass Failure(Exception):\n    pass\n",
            ),
        ],
    );
    let hierarchy = index.hierarchy();
    let embedding = class(&index, "app/embeddings.py", "EmbeddingProcess");
    assert_eq!(
        hierarchy.repository_base(embedding),
        Some(class(&index, "app/util/process.py", "AppProcess"))
    );
    assert_eq!(
        hierarchy.external_root(embedding),
        Some("multiprocessing.Process")
    );
    assert_eq!(hierarchy.depth(embedding), 3);
    assert_eq!(
        target(&index, "app/worker.py", "Worker"),
        BaseTarget::External("multiprocessing.Process".to_string())
    );
    assert_eq!(
        target(&index, "app/worker.py", "Failure"),
        BaseTarget::External("Exception".to_string())
    );
}

#[test]
fn an_unimported_python_name_never_resolves_to_another_files_class() {
    let index = index_sources(
        "hierarchy-unimported",
        &[
            ("a/base.py", Language::Python, "class Base:\n    pass\n"),
            (
                "b/child.py",
                Language::Python,
                "class Child(Base):\n    pass\n",
            ),
        ],
    );
    assert_eq!(
        target(&index, "b/child.py", "Child"),
        BaseTarget::External("Base".to_string())
    );
}

#[test]
fn a_reexported_base_keeps_its_import_file_but_no_type() {
    let index = index_sources(
        "hierarchy-reexport",
        &[
            ("app/api.py", Language::Python, "from .impl import Base\n"),
            ("app/impl.py", Language::Python, "class Base:\n    pass\n"),
            (
                "app/main.py",
                Language::Python,
                "from .api import Base\n\n\nclass Child(Base):\n    pass\n",
            ),
        ],
    );
    let base = index
        .hierarchy()
        .base(class(&index, "app/main.py", "Child"))
        .cloned()
        .expect("base");
    assert_eq!(base.target, BaseTarget::Unknown);
    assert_eq!(
        base.import_file
            .map(|file| index.files()[file].path.as_str()),
        Some("app/api.py")
    );
}

#[test]
fn java_sees_types_of_its_package_without_an_import() {
    let index = index_sources(
        "hierarchy-java",
        &[
            (
                "src/p/Base.java",
                Language::Java,
                "package p;\nclass Base {}\n",
            ),
            (
                "src/p/Child.java",
                Language::Java,
                "package p;\nclass Child extends Base {}\n",
            ),
            (
                "src/p/Leaf.java",
                Language::Java,
                "package p;\nclass Leaf extends java.util.AbstractList {}\n",
            ),
        ],
    );
    assert_eq!(
        target(&index, "src/p/Child.java", "Child"),
        BaseTarget::Repository(class(&index, "src/p/Base.java", "Base"))
    );
    assert_eq!(
        target(&index, "src/p/Leaf.java", "Leaf"),
        BaseTarget::External("java.util.AbstractList".to_string())
    );
}

#[test]
fn an_unresolved_relative_import_is_not_external() {
    let index = index_sources(
        "hierarchy-relative",
        &[(
            "app/main.py",
            Language::Python,
            "from .missing import Base\n\n\nclass Child(Base):\n    pass\n",
        )],
    );
    assert_eq!(target(&index, "app/main.py", "Child"), BaseTarget::Unknown);
}

#[test]
fn a_cyclic_hierarchy_ends() {
    let index = index_sources(
        "hierarchy-cycle",
        &[(
            "a.py",
            Language::Python,
            "class A(B):\n    pass\n\n\nclass B(A):\n    pass\n",
        )],
    );
    let a = class(&index, "a.py", "A");
    assert_eq!(index.hierarchy().lineage(a).len(), 2);
    assert_eq!(index.hierarchy().depth(a), 1);
}
