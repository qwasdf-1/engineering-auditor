use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_REPOSITORY: AtomicU64 = AtomicU64::new(0);

pub(super) struct TempRepo {
    root: PathBuf,
}

impl TempRepo {
    pub(super) fn new(name: &str) -> Self {
        let sequence = NEXT_REPOSITORY.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock precedes Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "engineering-auditor-{name}-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create temporary test repository");
        Self { root }
    }

    pub(super) fn path(&self) -> &Path {
        &self.root
    }

    pub(super) fn write(&self, relative: &str, text: impl AsRef<[u8]>) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create temporary repository parent");
        }
        fs::write(path, text).expect("write temporary repository file");
    }

    pub(super) fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap_or_else(|error| panic!("run git {args:?}: {error}"));
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl AsRef<Path> for TempRepo {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl Deref for TempRepo {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.path()
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) fn temp_root(name: &str) -> TempRepo {
    TempRepo::new(name)
}

pub(super) fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create temporary repository parent");
    }
    fs::write(path, text).expect("write temporary repository file");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_repository_when_guard_drops() {
        let path = {
            let repository = TempRepo::new("drop");
            repository.write("src/lib.rs", "fn example() {}\n");
            repository.path().to_path_buf()
        };

        assert!(!path.exists());
    }
}
