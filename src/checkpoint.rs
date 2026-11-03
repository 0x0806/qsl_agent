//! File checkpoints: before a tool mutates a file, its previous content is
//! copied aside so `/rollback` can undo the last turn's file changes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct CheckpointEntry {
    pub turn: u64,
    pub path: PathBuf,
    /// `None` when the file did not exist before (rollback deletes it).
    pub backup: Option<PathBuf>,
}

pub struct Checkpoints {
    dir: PathBuf,
    entries: Mutex<Vec<CheckpointEntry>>,
}

impl Checkpoints {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Record `path`'s current state once per turn, before it is modified.
    pub fn snapshot(&self, path: &Path, turn: u64) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if entries.iter().any(|e| e.turn == turn && e.path == path) {
            return;
        }
        let backup = if path.is_file() {
            let name = format!(
                "{}-{}-{}",
                turn,
                entries.len(),
                path.file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".into())
            );
            let dst = self.dir.join(name);
            match fs::create_dir_all(&self.dir).and_then(|_| fs::copy(path, &dst)) {
                Ok(_) => Some(dst),
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "checkpoint copy failed");
                    return;
                }
            }
        } else {
            None
        };
        entries.push(CheckpointEntry {
            turn,
            path: path.to_path_buf(),
            backup,
        });
    }

    pub fn list(&self) -> Vec<CheckpointEntry> {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Restore every file touched in the most recent turn that has
    /// checkpoints. Returns the restored paths.
    pub fn rollback_last(&self) -> Result<Vec<String>, String> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let Some(turn) = entries.last().map(|e| e.turn) else {
            return Err("no file changes recorded in this session".into());
        };
        let (undo, keep): (Vec<_>, Vec<_>) = entries.drain(..).partition(|e| e.turn == turn);
        *entries = keep;
        let mut restored = Vec::new();
        for e in undo.into_iter().rev() {
            match &e.backup {
                Some(b) => fs::copy(b, &e.path)
                    .map(|_| ())
                    .map_err(|err| format!("{}: {err}", e.path.display()))?,
                None => {
                    if e.path.exists() {
                        fs::remove_file(&e.path)
                            .map_err(|err| format!("{}: {err}", e.path.display()))?;
                    }
                }
            }
            restored.push(format!(
                "{} ({})",
                e.path.display(),
                if e.backup.is_some() {
                    "restored"
                } else {
                    "removed"
                }
            ));
        }
        Ok(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_restores_and_removes() {
        let d = tempfile::tempdir().unwrap();
        let cp = Checkpoints::new(d.path().join("cp"));
        let a = d.path().join("a.txt");
        let b = d.path().join("b.txt");
        fs::write(&a, "original").unwrap();
        cp.snapshot(&a, 1);
        cp.snapshot(&b, 1);
        fs::write(&a, "changed").unwrap();
        fs::write(&b, "new file").unwrap();
        cp.snapshot(&a, 1); // second snapshot in same turn is ignored
        let restored = cp.rollback_last().unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(fs::read_to_string(&a).unwrap(), "original");
        assert!(!b.exists());
        assert!(cp.rollback_last().is_err());
    }
}
