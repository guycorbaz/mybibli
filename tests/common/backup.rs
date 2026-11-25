//! Story 19-6 — helpers shared by the backup tests: a backup folder and a
//! covers folder of their own, the archive read back member by member.
#![allow(dead_code)] // each test file uses a different subset

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use mybibli::services::backup::BackupConfig;
use mybibli::services::backup::archive::{Manifest, hex};
use sha2::{Digest, Sha256};

/// A temporary root holding `backups/` (the mounted folder) and `covers/`.
pub struct Folders {
    pub root: tempfile::TempDir,
}

impl Folders {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("backups")).unwrap();
        std::fs::create_dir(root.path().join("covers")).unwrap();
        Folders { root }
    }

    pub fn backups(&self) -> PathBuf {
        self.root.path().join("backups")
    }

    pub fn covers(&self) -> PathBuf {
        self.root.path().join("covers")
    }

    pub fn config(&self) -> BackupConfig {
        BackupConfig::new(self.backups(), self.covers())
    }
}

/// The names in a folder, sorted.
pub fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The members of a `.tar.gz`, in order, with their bytes.
pub fn read_members(archive: &Path) -> Vec<(String, Vec<u8>)> {
    let file = std::fs::File::open(archive).unwrap();
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut out = Vec::new();
    for entry in ar.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().to_string();
        let mut body = Vec::new();
        entry.read_to_end(&mut body).unwrap();
        out.push((name, body));
    }
    out
}

/// The manifest of a read archive (its last member).
pub fn manifest_of(members: &[(String, Vec<u8>)]) -> Manifest {
    let (name, body) = members.last().expect("a manifest");
    assert_eq!(name, "manifest.json", "the manifest is the last member");
    serde_json::from_slice(body).unwrap()
}

/// Every member before the manifest matches the hash and size it lists.
pub fn assert_members_match(members: &[(String, Vec<u8>)], manifest: &Manifest) {
    let listed: BTreeMap<&str, (&str, u64)> = manifest
        .members
        .iter()
        .map(|m| (m.name.as_str(), (m.sha256.as_str(), m.size)))
        .collect();
    assert_eq!(listed.len(), members.len() - 1, "every member is listed once");
    for (name, body) in &members[..members.len() - 1] {
        let (sha, size) = listed.get(name.as_str()).unwrap_or_else(|| panic!("{name} listed"));
        assert_eq!(*size, body.len() as u64, "{name} size");
        assert_eq!(*sha, sha256_hex(body), "{name} hash");
    }
}

/// `sha256sum -c <sidecar>` run in the folder; `None` without the tool.
pub fn sha256sum_check(dir: &Path, sidecar: &str) -> Option<bool> {
    let out = std::process::Command::new("sha256sum")
        .arg("-c")
        .arg("--quiet")
        .arg(sidecar)
        .current_dir(dir)
        .output()
        .ok()?;
    Some(out.status.success())
}
