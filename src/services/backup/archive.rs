//! Story 19-6 (D2) — the `.tar.gz` stream and its manifest.
//!
//! Members, in order: `database.sql`, `covers/<name>` (one per cover),
//! `manifest.json` last. Every member is written `0600` in the tar header.
//! The manifest lists the SHA-256 and size of each member before it; the
//! SHA-256 of the whole archive file goes to the `.sha256` sidecar
//! (`sha256sum -c`), so a truncated file is detected before it is opened.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::covers::CoverList;
use super::{BackupError, FORMAT_NAME, FORMAT_VERSION};

/// Name of the SQL member.
pub const SQL_MEMBER: &str = "database.sql";
/// Name of the manifest member (last).
pub const MANIFEST_MEMBER: &str = "manifest.json";
/// Folder of the cover members.
pub const COVERS_PREFIX: &str = "covers/";

/// Lowercase hex of a byte string.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 (hex) and size of what went through a [`HashingWriter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    pub sha256: String,
    pub size: u64,
}

/// A writer that hashes and counts what it passes on.
pub struct HashingWriter<W> {
    inner: W,
    hasher: Sha256,
    bytes: u64,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        HashingWriter {
            inner,
            hasher: Sha256::new(),
            bytes: 0,
        }
    }

    /// The inner writer, and the digest of everything written.
    pub fn finish(self) -> (W, Digest) {
        let digest = Digest {
            sha256: hex(&self.hasher.finalize()),
            size: self.bytes,
        };
        (self.inner, digest)
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// One member as the manifest lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberEntry {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

/// The covers part of the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverSummary {
    pub count: u64,
    pub bytes: u64,
    /// Entries of `COVERS_DIR` that are not `<digits>.jpg` files
    /// (`.staging/` included).
    pub skipped: u64,
}

/// `manifest.json`, format version 1 (read by story 19-8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub instance_id: String,
    pub mybibli_version: String,
    pub last_migration: Option<i64>,
    /// UTC, RFC 3339.
    pub created_at: String,
    /// Row count of each table.
    pub tables: BTreeMap<String, u64>,
    pub covers: CoverSummary,
    /// Every member before the manifest, in archive order.
    pub members: Vec<MemberEntry>,
}

/// What the manifest takes from the dump and the clock.
#[derive(Debug, Clone)]
pub struct ManifestHead {
    pub instance_id: String,
    pub mybibli_version: String,
    pub last_migration: Option<i64>,
    pub created_at: String,
    /// Modification time of the members (Unix seconds).
    pub mtime: u64,
    pub tables: BTreeMap<String, u64>,
}

/// What the archive writer produced.
#[derive(Debug, Clone)]
pub struct ArchiveOutcome {
    /// SHA-256 of the whole `.tar.gz` file (the sidecar line).
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub cover_count: u64,
    pub cover_bytes: u64,
}

fn append<W: Write, R: Read>(
    builder: &mut tar::Builder<W>,
    name: &str,
    size: u64,
    mtime: u64,
    data: R,
) -> std::io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(size);
    header.set_mode(0o600);
    header.set_mtime(mtime);
    builder.append_data(&mut header, name, data)
}

/// Write the archive into `file` (created `0600` by the caller), then
/// flush and `sync_all` it.
pub fn write_archive(
    file: File,
    sql_path: &Path,
    sql_digest: Digest,
    covers: &CoverList,
    head: ManifestHead,
) -> Result<ArchiveOutcome, BackupError> {
    let counted = HashingWriter::new(file);
    let gz = GzEncoder::new(counted, Compression::default());
    let mut builder = tar::Builder::new(gz);
    let mut members = Vec::with_capacity(covers.files.len() + 1);

    // 1. database.sql, from the scratch file (its digest was computed
    //    while it was written).
    let sql = File::open(sql_path)?;
    let on_disk = sql.metadata()?.len();
    if on_disk != sql_digest.size {
        return Err(BackupError::Io(format!(
            "dump size changed: {on_disk} on disk, {} written",
            sql_digest.size
        )));
    }
    append(&mut builder, SQL_MEMBER, sql_digest.size, head.mtime, sql)?;
    members.push(MemberEntry {
        name: SQL_MEMBER.to_string(),
        sha256: sql_digest.sha256,
        size: sql_digest.size,
    });

    // 2. Covers: read whole (they are small), so the size in the header
    //    and the bytes hashed are the bytes written. One that vanished
    //    since the listing is skipped.
    let mut summary = CoverSummary {
        count: 0,
        bytes: 0,
        skipped: covers.skipped,
    };
    for cover in &covers.files {
        let (name, path) = (&cover.name, &cover.path);
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                summary.skipped += 1;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let member = format!("{COVERS_PREFIX}{name}");
        append(&mut builder, &member, bytes.len() as u64, head.mtime, bytes.as_slice())?;
        members.push(MemberEntry {
            name: member,
            sha256: hex(&Sha256::digest(&bytes)),
            size: bytes.len() as u64,
        });
        summary.count += 1;
        summary.bytes += bytes.len() as u64;
    }

    // 3. The manifest, last.
    let manifest = Manifest {
        format: FORMAT_NAME.to_string(),
        format_version: FORMAT_VERSION,
        instance_id: head.instance_id,
        mybibli_version: head.mybibli_version,
        last_migration: head.last_migration,
        created_at: head.created_at,
        tables: head.tables,
        covers: summary.clone(),
        members,
    };
    let json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| BackupError::Io(format!("manifest serialisation: {e}")))?;
    append(&mut builder, MANIFEST_MEMBER, json.len() as u64, head.mtime, json.as_slice())?;

    let gz = builder.into_inner()?;
    let counted = gz.finish()?;
    let (mut file, digest) = counted.finish();
    file.flush()?;
    file.sync_all()?;
    Ok(ArchiveOutcome {
        archive_sha256: digest.sha256,
        archive_bytes: digest.size,
        cover_count: summary.count,
        cover_bytes: summary.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;

    #[test]
    fn the_hashing_writer_counts_and_hashes() {
        let mut w = HashingWriter::new(Vec::new());
        w.write_all(b"abc").unwrap();
        let (inner, d) = w.finish();
        assert_eq!(inner, b"abc");
        assert_eq!(d.size, 3);
        assert_eq!(
            d.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_archive_holds_the_dump_the_covers_and_the_manifest_last() {
        let tmp = tempfile::tempdir().unwrap();
        let sql_path = tmp.path().join("d.sql");
        let mut w = HashingWriter::new(Vec::new());
        w.write_all(b"SET NAMES utf8mb4;\n").unwrap();
        let (sql, sql_digest) = w.finish();
        std::fs::write(&sql_path, &sql).unwrap();
        let covers_dir = tmp.path().join("covers");
        std::fs::create_dir(&covers_dir).unwrap();
        std::fs::write(covers_dir.join("7.jpg"), b"jpeg").unwrap();
        let covers = super::super::covers::list_covers(&covers_dir).unwrap();
        let out = tmp.path().join("a.tar.gz");
        let head = ManifestHead {
            instance_id: "0123456789abcdef".into(),
            mybibli_version: "1.2.3".into(),
            last_migration: Some(42),
            created_at: "2026-11-25T07:04:09Z".into(),
            mtime: 1_700_000_000,
            tables: BTreeMap::from([("titles".to_string(), 3)]),
        };
        let outcome = write_archive(
            File::create(&out).unwrap(),
            &sql_path,
            sql_digest.clone(),
            &covers,
            head,
        )
        .unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(outcome.archive_bytes, bytes.len() as u64);
        assert_eq!(outcome.archive_sha256, hex(&Sha256::digest(&bytes)));
        assert_eq!(outcome.cover_count, 1);

        let mut ar = tar::Archive::new(GzDecoder::new(bytes.as_slice()));
        let mut names = Vec::new();
        let mut manifest: Option<Manifest> = None;
        for entry in ar.entries().unwrap() {
            let mut entry = entry.unwrap();
            assert_eq!(entry.header().mode().unwrap(), 0o600);
            let name = entry.path().unwrap().to_string_lossy().to_string();
            let mut body = Vec::new();
            entry.read_to_end(&mut body).unwrap();
            if name == MANIFEST_MEMBER {
                manifest = Some(serde_json::from_slice(&body).unwrap());
            }
            names.push(name);
        }
        assert_eq!(names, vec!["database.sql", "covers/7.jpg", "manifest.json"]);
        let m = manifest.unwrap();
        assert_eq!(m.format, "mybibli-backup");
        assert_eq!(m.format_version, 1);
        assert_eq!(m.last_migration, Some(42));
        assert_eq!(m.covers.count, 1);
        assert_eq!(m.covers.bytes, 4);
        assert_eq!(m.members[0].sha256, sql_digest.sha256);
        assert_eq!(m.members[1].sha256, hex(&Sha256::digest(b"jpeg")));
    }
}
