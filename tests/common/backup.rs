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

// ─── Story 19-8 — shared by the round trip and the restore tests ─────

/// The base tables of `schema` (`DATABASE()` when `None`).
pub async fn table_names(pool: &sqlx::MySqlPool, schema: Option<&str>) -> std::collections::BTreeSet<String> {
    let rows: Vec<(String,)> = match schema {
        Some(s) => sqlx::query_as(
            "SELECT CAST(TABLE_NAME AS CHAR) FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'",
        )
        .bind(s)
        .fetch_all(pool)
        .await
        .unwrap(),
        None => sqlx::query_as(
            "SELECT CAST(TABLE_NAME AS CHAR) FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE'",
        )
        .fetch_all(pool)
        .await
        .unwrap(),
    };
    rows.into_iter().map(|(n,)| n).collect()
}

/// `CHECKSUM TABLE` and the row count of `schema.table`.
pub async fn checksum(pool: &sqlx::MySqlPool, schema: &str, table: &str) -> (u64, i64) {
    use sqlx::Row;
    let row = sqlx::query(&format!("CHECKSUM TABLE `{schema}`.`{table}`"))
        .fetch_one(pool)
        .await
        .unwrap();
    let sum = row
        .try_get::<Option<u64>, _>(1)
        .or_else(|_| row.try_get::<Option<i64>, _>(1).map(|v| v.map(|x| x as u64)))
        .unwrap()
        .unwrap_or_else(|| panic!("no checksum for {schema}.{table}"));
    let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM `{schema}`.`{table}`"))
        .fetch_one(pool)
        .await
        .unwrap();
    (sum, count)
}

/// Every table of the current schema with its (checksum, rows), except
/// `skip`.
pub async fn library_state(
    pool: &sqlx::MySqlPool,
    skip: &[&str],
) -> BTreeMap<String, (u64, i64)> {
    let schema: String = sqlx::query_scalar("SELECT DATABASE()").fetch_one(pool).await.unwrap();
    let mut out = BTreeMap::new();
    for t in table_names(pool, None).await {
        if skip.contains(&t.as_str()) {
            continue;
        }
        let c = checksum(pool, &schema, &t).await;
        out.insert(t, c);
    }
    out
}

/// One archive member with a raw name (any bytes, `../x` included: the
/// name is written into the header directly, as a hostile tool would).
fn append_raw<W: std::io::Write>(b: &mut tar::Builder<W>, name: &[u8], body: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(body.len() as u64);
    header.set_mode(0o600);
    header.set_mtime(1_700_000_000);
    {
        let gnu = header.as_gnu_mut().unwrap();
        gnu.name = [0; 100];
        gnu.name[..name.len()].copy_from_slice(name);
    }
    header.set_cksum();
    b.append(&header, body).unwrap();
}

/// Write `members` (manifest excluded) + `manifest` (last) as
/// `dir/name`, with a correct sidecar. With `recompute`, the manifest's
/// member list is rebuilt from the bytes (a consistent forgery);
/// otherwise it is written as given.
pub fn write_archive(
    dir: &Path,
    name: &str,
    members: &[(Vec<u8>, Vec<u8>)],
    manifest: &mut Manifest,
    recompute: bool,
) {
    use std::io::Write;
    if recompute {
        manifest.members = members
            .iter()
            .map(|(n, body)| mybibli::services::backup::archive::MemberEntry {
                name: String::from_utf8_lossy(n).into_owned(),
                sha256: sha256_hex(body),
                size: body.len() as u64,
            })
            .collect();
    }
    let mut bytes = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut bytes, flate2::Compression::default());
        let mut b = tar::Builder::new(gz);
        for (n, body) in members {
            append_raw(&mut b, n, body);
        }
        append_raw(&mut b, b"manifest.json", &serde_json::to_vec_pretty(manifest).unwrap());
        b.into_inner().unwrap().finish().unwrap().flush().unwrap();
    }
    std::fs::write(dir.join(name), &bytes).unwrap();
    std::fs::write(
        dir.join(format!("{name}.sha256")),
        mybibli::services::backup::sidecar_line(&sha256_hex(&bytes), name),
    )
    .unwrap();
}

/// Read `src`, let `edit` change its members and manifest, and write the
/// result as `dir/name` (manifest member list recomputed when
/// `recompute`), with a fresh sidecar.
pub fn rebuild(
    src: &Path,
    dir: &Path,
    name: &str,
    recompute: bool,
    edit: impl FnOnce(&mut Vec<(Vec<u8>, Vec<u8>)>, &mut Manifest),
) {
    let read = read_members(src);
    let mut manifest = manifest_of(&read);
    let mut members: Vec<(Vec<u8>, Vec<u8>)> = read[..read.len() - 1]
        .iter()
        .map(|(n, b)| (n.as_bytes().to_vec(), b.clone()))
        .collect();
    edit(&mut members, &mut manifest);
    write_archive(dir, name, &members, &mut manifest, recompute);
}

/// A backup name of `id` at a fixed, real stamp.
pub fn name_of(id: &str, stamp: &str) -> String {
    format!("mybibli-{id}-{stamp}.tar.gz")
}
