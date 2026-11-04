//! Modules: the directories a repository divides into, chosen so that no
//! module holds more than a third of the files while it can still split.

use std::collections::{BTreeMap, BTreeSet};

/// A split stops before the partition passes this many modules.
const MAX_MODULES: usize = 64;

pub(super) struct Modules {
    names: Vec<String>,
    of_file: Vec<usize>,
    members: Vec<Vec<usize>>,
}

impl Modules {
    /// Partitions files by directory: the top-level directories first, then
    /// the largest module that holds more than a third of the files and has
    /// subdirectories splits into them (its own files stay), until none
    /// qualifies. Files at the root form the module `.`.
    pub(super) fn partition<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let directories = paths
            .into_iter()
            .map(|path| {
                let mut segments = path.split('/').collect::<Vec<_>>();
                segments.pop();
                segments
            })
            .collect::<Vec<_>>();
        let mut depths = directories
            .iter()
            .map(|segments| segments.len().min(1))
            .collect::<Vec<_>>();
        let key = |file: usize, depths: &[usize]| directories[file][..depths[file]].join("/");
        loop {
            let mut sizes: BTreeMap<String, (usize, bool)> = BTreeMap::new();
            for file in 0..directories.len() {
                let entry = sizes.entry(key(file, &depths)).or_default();
                entry.0 += 1;
                entry.1 |= directories[file].len() > depths[file];
            }
            let Some(split) = sizes
                .iter()
                .filter(|(_, (size, splittable))| *splittable && size * 3 > directories.len())
                .max_by(|left, right| left.1 .0.cmp(&right.1 .0).then(right.0.cmp(left.0)))
                .map(|(name, _)| name.clone())
            else {
                break;
            };
            let deeper = (0..directories.len())
                .filter(|file| {
                    key(*file, &depths) == split && directories[*file].len() > depths[*file]
                })
                .collect::<Vec<_>>();
            let children = deeper
                .iter()
                .map(|file| directories[*file][depths[*file]])
                .collect::<BTreeSet<_>>();
            if sizes.len() + children.len() > MAX_MODULES {
                break;
            }
            for file in deeper {
                depths[file] += 1;
            }
        }
        let names = (0..directories.len())
            .map(|file| key(file, &depths))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|name| {
                if name.is_empty() {
                    ".".to_string()
                } else {
                    name
                }
            })
            .collect::<Vec<_>>();
        let index = names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.clone(), index))
            .collect::<BTreeMap<_, _>>();
        let of_file = (0..directories.len())
            .map(|file| {
                let name = key(file, &depths);
                index[if name.is_empty() { "." } else { name.as_str() }]
            })
            .collect::<Vec<_>>();
        let mut members = vec![Vec::new(); names.len()];
        for (file, module) in of_file.iter().enumerate() {
            members[*module].push(file);
        }
        Self {
            names,
            of_file,
            members,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.names.len()
    }

    pub(super) fn name(&self, module: usize) -> &str {
        &self.names[module]
    }

    pub(super) fn id(&self, module: usize) -> String {
        format!("module:{}", self.names[module])
    }

    pub(super) fn of_file(&self, file: usize) -> usize {
        self.of_file[file]
    }

    pub(super) fn members(&self, module: usize) -> &[usize] {
        &self.members[module]
    }

    /// A module by its evidence ID or name.
    pub(super) fn find(&self, query: &str) -> Option<usize> {
        let name = query.strip_prefix("module:").unwrap_or(query);
        let name = name.trim_end_matches('/');
        self.names.iter().position(|candidate| candidate == name)
    }
}

/// The module dependency graph's cycles: each strongly connected component
/// of more than one module, members sorted, components in order.
pub(super) fn cycles(
    module_count: usize,
    edges: &BTreeMap<(usize, usize), usize>,
) -> Vec<Vec<usize>> {
    let mut successors = vec![Vec::new(); module_count];
    for (from, to) in edges.keys() {
        successors[*from].push(*to);
    }
    let mut tarjan = Tarjan {
        successors: &successors,
        index: vec![None; module_count],
        low: vec![0; module_count],
        on_stack: vec![false; module_count],
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    for module in 0..module_count {
        if tarjan.index[module].is_none() {
            tarjan.visit(module);
        }
    }
    let mut components = tarjan
        .components
        .into_iter()
        .filter(|component| component.len() > 1)
        .map(|mut component| {
            component.sort_unstable();
            component
        })
        .collect::<Vec<_>>();
    components.sort();
    components
}

struct Tarjan<'a> {
    successors: &'a [Vec<usize>],
    index: Vec<Option<usize>>,
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    next: usize,
    components: Vec<Vec<usize>>,
}

impl Tarjan<'_> {
    fn visit(&mut self, node: usize) {
        self.index[node] = Some(self.next);
        self.low[node] = self.next;
        self.next += 1;
        self.stack.push(node);
        self.on_stack[node] = true;
        for &successor in &self.successors[node] {
            match self.index[successor] {
                None => {
                    self.visit(successor);
                    self.low[node] = self.low[node].min(self.low[successor]);
                }
                Some(index) if self.on_stack[successor] => {
                    self.low[node] = self.low[node].min(index);
                }
                Some(_) => {}
            }
        }
        if Some(self.low[node]) == self.index[node] {
            let mut component = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.on_stack[member] = false;
                component.push(member);
                if member == node {
                    break;
                }
            }
            self.components.push(component);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_largest_module_splits_until_none_holds_a_third() {
        let modules = Modules::partition([
            "README.md",
            "src/main/java/org/app/api/Routes.java",
            "src/main/java/org/app/api/Handlers.java",
            "src/main/java/org/app/store/Db.java",
            "src/main/java/org/app/store/Cache.java",
            "src/main/java/org/app/App.java",
            "docs/guide.md",
        ]);
        let names = (0..modules.len())
            .map(|module| modules.name(module))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                ".",
                "docs",
                "src/main/java/org/app",
                "src/main/java/org/app/api",
                "src/main/java/org/app/store"
            ]
        );
        assert_eq!(modules.find("module:docs"), Some(1));
        assert_eq!(modules.find("src/main/java/org/app/api/"), Some(3));
    }

    #[test]
    fn cycles_are_strongly_connected_components() {
        let edges = BTreeMap::from([((0, 1), 2), ((1, 2), 1), ((2, 0), 1), ((3, 0), 4)]);
        assert_eq!(cycles(4, &edges), vec![vec![0, 1, 2]]);
    }
}
