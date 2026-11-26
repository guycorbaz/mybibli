//! Story 19-8 (#657, D5) — the checks a backup passes before anything is
//! touched.
//!
//! One read of the file, in a blocking task: the whole file is hashed
//! (sidecar), the gzip stream and the tar members are read on the way,
//! every member is hashed and sized. Positive shapes only (Rule #20): the
//! members are exactly `database.sql` first, `covers/<digits>.jpg`…, and
//! `manifest.json` last, regular entries only — any other name, a link, a
//! directory, a path with `/` or `..` beyond that shape is refused. Nothing
//! is extracted here.
//!
//! The order of the verdicts: a file whose bytes do not match its sidecar
//! is damaged (R1) whatever else is wrong with it; an intact file whose
//! members differ from its manifest is damaged too (rebuilt by hand); an
//! intact file of another shape, format or instance is "not a backup this
//! version can restore" (R5); a known shape at a migration level above this
//! binary's is "newer" (R3).

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use sha2::{Digest as _, Sha256};

use super::Refusal;
use crate::services::backup::archive::{
    COVERS_PREFIX, MANIFEST_MEMBER, Manifest, MemberEntry, SQL_MEMBER, hex,
};
use crate::services::backup::covers::is_cover_name;
use crate::services::backup::{FORMAT_NAME, FORMAT_VERSION, listing, sidecar_path};

/// The manifest is small (one line per cover); a larger one is not ours.
pub const MANIFEST_MAX_BYTES: u64 = 64 << 20;

/// What a backup member is, by its name (positive shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Member {
    Sql,
    /// A cover: its file name in `COVERS_DIR` (`<digits>.jpg`).
    Cover(String),
    Manifest,
}

/// The kind of the member named `name`; `None` for any other name.
pub fn member_kind(name: &[u8]) -> Option<Member> {
    let name = std::str::from_utf8(name).ok()?;
    if name == SQL_MEMBER {
        return Some(Member::Sql);
    }
    if name == MANIFEST_MEMBER {
        return Some(Member::Manifest);
    }
    let file = name.strip_prefix(COVERS_PREFIX)?;
    is_cover_name(file).then(|| Member::Cover(file.to_string()))
}

/// The member order: `database.sql` first, covers (each once), the
/// manifest last and nothing after it.
#[derive(Debug, Default)]
pub struct MemberOrder {
    count: usize,
    manifest_seen: bool,
    covers: HashSet<String>,
}

impl MemberOrder {
    /// Whether `member` may come next.
    pub fn accept(&mut self, member: &Member) -> bool {
        let ok = match member {
            _ if self.manifest_seen => false,
            Member::Sql => self.count == 0,
            Member::Cover(name) => self.count > 0 && self.covers.insert(name.clone()),
            Member::Manifest => self.count > 0,
        };
        if ok {
            self.count += 1;
            if *member == Member::Manifest {
                self.manifest_seen = true;
            }
        }
        ok
    }

    /// The archive ended: it held the dump and the manifest.
    pub fn is_complete(&self) -> bool {
        self.manifest_seen
    }
}

/// A reader that hashes and counts what goes through it.
pub struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
    bytes: u64,
}

impl<R: Read> HashingReader<R> {
    pub fn new(inner: R) -> Self {
        HashingReader {
            inner,
            hasher: Sha256::new(),
            bytes: 0,
        }
    }

    /// SHA-256 (hex) and size of everything read.
    pub fn finish(self) -> (String, u64) {
        (hex(&self.hasher.finalize()), self.bytes)
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
}

/// The SHA-256 a sidecar names for `file_name`: one `sha256sum` line,
/// `<64 lowercase hex>  <file_name>`, optional final newline.
pub fn parse_sidecar(text: &str, file_name: &str) -> Option<String> {
    let line = text.strip_suffix('\n').unwrap_or(text);
    let (hash, name) = line.split_once("  ")?;
    let shaped = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    (shaped && name == file_name).then(|| hash.to_string())
}

/// D5's rules on the manifest: the known format, the id of the name, a
/// migration level this binary knows (above its newest: R3; an unknown
/// lower level: R5).
pub fn check_manifest(
    manifest: &Manifest,
    name_id: &str,
    newest_known: Option<i64>,
    is_known: impl Fn(i64) -> bool,
    current_version: &str,
) -> Result<(), Refusal> {
    if manifest.format != FORMAT_NAME || manifest.format_version != FORMAT_VERSION {
        return Err(Refusal::NotABackup);
    }
    if manifest.instance_id != name_id {
        return Err(Refusal::NotABackup);
    }
    let Some(level) = manifest.last_migration else {
        return Err(Refusal::NotABackup);
    };
    if newest_known.is_some_and(|newest| level > newest) {
        return Err(Refusal::Newer {
            version: manifest.mybibli_version.clone(),
            current: current_version.to_string(),
        });
    }
    if !is_known(level) {
        return Err(Refusal::NotABackup);
    }
    Ok(())
}

/// A backup that passed every check.
#[derive(Debug, Clone)]
pub struct Verified {
    pub file_name: String,
    /// The instance id in the name (= the manifest's).
    pub instance_id: String,
    /// The UTC time in the name.
    pub stamp: DateTime<Utc>,
    /// Size of the archive in bytes.
    pub size: u64,
    pub manifest: Manifest,
    /// Review pass 1 (TOCTOU): the file as it was opened and checked
    /// (`O_NOFOLLOW`); the load reads this descriptor, never the path again.
    pub file: std::sync::Arc<File>,
}

impl Verified {
    /// Written at an older migration level than this binary's newest: it
    /// is brought up to date after the load.
    pub fn is_older(&self) -> bool {
        match (self.manifest.last_migration, crate::db::newest_known_migration()) {
            (Some(level), Some(newest)) => level < newest,
            _ => false,
        }
    }
}

/// What reading the members found (before the sidecar verdict).
enum Scan {
    Members { members: Vec<MemberEntry>, manifest: Vec<u8> },
    /// Not the backup shape (name, order, entry type, a member too large).
    Shape,
    /// The gzip / tar stream could not be read.
    Unreadable,
}

fn scan_members<R: Read>(archive: &mut tar::Archive<R>) -> Scan {
    let entries = match archive.entries() {
        Ok(e) => e,
        Err(_) => return Scan::Unreadable,
    };
    let mut order = MemberOrder::default();
    let mut members = Vec::new();
    let mut manifest = Vec::new();
    for entry in entries {
        let mut entry = match entry {
            Ok(e) => e,
            Err(_) => return Scan::Unreadable,
        };
        if entry.header().entry_type() != tar::EntryType::Regular {
            return Scan::Shape;
        }
        let name = entry.path_bytes().into_owned();
        let Some(kind) = member_kind(&name) else {
            return Scan::Shape;
        };
        if !order.accept(&kind) {
            return Scan::Shape;
        }
        let declared = match entry.header().size() {
            Ok(s) => s,
            Err(_) => return Scan::Shape,
        };
        if kind == Member::Manifest {
            if declared > MANIFEST_MAX_BYTES {
                return Scan::Shape;
            }
            if entry.read_to_end(&mut manifest).is_err() {
                return Scan::Unreadable;
            }
            continue;
        }
        let mut hashing = HashingReader::new(&mut entry);
        if std::io::copy(&mut hashing, &mut std::io::sink()).is_err() {
            return Scan::Unreadable;
        }
        let (sha256, size) = hashing.finish();
        members.push(MemberEntry {
            name: String::from_utf8_lossy(&name).into_owned(),
            sha256,
            size,
        });
    }
    if !order.is_complete() {
        return Scan::Shape;
    }
    Scan::Members { members, manifest }
}

/// Open the archive for the checks and the load (review passes 1 and 2):
/// `O_NOFOLLOW` (a link swapped in after the `stat` fails with ELOOP:
/// "not in the folder"), `O_NONBLOCK` (a FIFO never blocks the open), then
/// `fstat` must say a regular file. Not found → gone; any other failure →
/// not readable.
pub fn open_checked(path: &Path, name: &str) -> Result<File, Refusal> {
    use std::os::unix::fs::OpenOptionsExt;
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path);
    let file = match opened {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Refusal::Gone),
        Err(e) if e.raw_os_error() == Some(nix::libc::ELOOP) => return Err(Refusal::Gone),
        Err(_) => return Err(Refusal::NotReadable { file: name.to_string() }),
    };
    if !file.metadata().map(|m| m.is_file()).unwrap_or(false) {
        return Err(Refusal::Gone);
    }
    Ok(file)
}

/// Every D5 check on `dir/name` (blocking: run it in `spawn_blocking`).
pub fn verify_backup(dir: &Path, name: &str) -> Result<Verified, Refusal> {
    let Some((name_id, stamp)) = listing::parse_backup_name(name) else {
        return Err(Refusal::Gone);
    };
    let path = dir.join(name);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(Refusal::NotReadable { file: name.to_string() });
        }
        Err(_) => return Err(Refusal::Gone),
    };
    if !meta.file_type().is_file() {
        return Err(Refusal::Gone);
    }
    let file = open_checked(&path, name)?;
    let file = std::sync::Arc::new(file);
    let sidecar = sidecar_path(dir, name);
    let sidecar_name = format!("{name}.sha256");
    match std::fs::symlink_metadata(&sidecar) {
        Ok(m) if m.file_type().is_file() => {}
        Ok(_) => return Err(Refusal::NoSidecar { file: name.to_string() }),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(Refusal::NotReadable { file: sidecar_name });
        }
        Err(_) => return Err(Refusal::NoSidecar { file: name.to_string() }),
    }
    let sidecar_text = match std::fs::read(&sidecar) {
        Ok(bytes) if bytes.len() <= 4096 => String::from_utf8(bytes).unwrap_or_default(),
        Ok(_) => String::new(),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(Refusal::NotReadable { file: sidecar_name });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Refusal::NoSidecar { file: name.to_string() });
        }
        Err(_) => return Err(Refusal::Damaged),
    };
    let Some(expected_sha) = parse_sidecar(&sidecar_text, name) else {
        return Err(Refusal::Damaged);
    };

    // One read: the whole file through a hasher, the stream decoded on the
    // way; whatever the tar reader leaves (end padding) is drained so the
    // hash covers every byte.
    let mut whole = HashingReader::new(BufReader::with_capacity(1 << 16, &*file));
    let scan = {
        let mut archive = tar::Archive::new(GzDecoder::new(&mut whole));
        scan_members(&mut archive)
    };
    let drained = std::io::copy(&mut whole, &mut std::io::sink());
    let (whole_sha, size) = whole.finish();
    if drained.is_err() || whole_sha != expected_sha {
        return Err(Refusal::Damaged);
    }
    let (members, manifest_bytes) = match scan {
        Scan::Members { members, manifest } => (members, manifest),
        Scan::Shape | Scan::Unreadable => return Err(Refusal::NotABackup),
    };
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|_| Refusal::NotABackup)?;
    // An intact file whose members are not those its manifest lists was
    // rebuilt or edited: damaged (the member hashes are the content's
    // checksums).
    if manifest.members != members {
        return Err(Refusal::Damaged);
    }
    check_manifest(
        &manifest,
        name_id,
        crate::db::newest_known_migration(),
        crate::db::is_known_migration,
        env!("CARGO_PKG_VERSION"),
    )?;
    Ok(Verified {
        file_name: name.to_string(),
        instance_id: name_id.to_string(),
        stamp,
        size,
        manifest,
        file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn only_the_three_member_shapes_are_known() {
        assert_eq!(member_kind(b"database.sql"), Some(Member::Sql));
        assert_eq!(member_kind(b"manifest.json"), Some(Member::Manifest));
        assert_eq!(member_kind(b"covers/42.jpg"), Some(Member::Cover("42.jpg".into())));
        for bad in [
            &b"../x"[..],
            b"covers/a.png",
            b"covers/../1.jpg",
            b"covers/1.jpg/",
            b"covers/sub/1.jpg",
            b"/database.sql",
            b"./database.sql",
            b"covers/.jpg",
            b"covers/1.JPG",
            b"covers/",
            b"other.sql",
            b"covers/\xff.jpg",
        ] {
            assert_eq!(member_kind(bad), None, "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn the_dump_comes_first_and_the_manifest_last() {
        let cover = |n: &str| Member::Cover(n.to_string());
        let mut o = MemberOrder::default();
        assert!(o.accept(&Member::Sql));
        assert!(o.accept(&cover("1.jpg")));
        assert!(!o.accept(&cover("1.jpg")), "a cover twice");
        assert!(o.accept(&cover("2.jpg")));
        assert!(!o.is_complete());
        assert!(o.accept(&Member::Manifest));
        assert!(o.is_complete());
        assert!(!o.accept(&cover("3.jpg")), "nothing after the manifest");
        assert!(!o.accept(&Member::Manifest));

        let mut o = MemberOrder::default();
        assert!(!o.accept(&cover("1.jpg")), "the dump first");
        assert!(!o.accept(&Member::Manifest));
        let mut o = MemberOrder::default();
        assert!(o.accept(&Member::Sql));
        assert!(!o.accept(&Member::Sql), "one dump");
    }

    #[test]
    fn a_sidecar_is_one_sha256sum_line_for_this_file() {
        let h = "a".repeat(64);
        assert_eq!(parse_sidecar(&format!("{h}  x.tar.gz\n"), "x.tar.gz"), Some(h.clone()));
        assert_eq!(parse_sidecar(&format!("{h}  x.tar.gz"), "x.tar.gz"), Some(h.clone()));
        assert_eq!(parse_sidecar(&format!("{h}  y.tar.gz\n"), "x.tar.gz"), None);
        assert_eq!(parse_sidecar(&format!("{}  x.tar.gz\n", "A".repeat(64)), "x.tar.gz"), None);
        assert_eq!(parse_sidecar(&format!("{h} x.tar.gz\n"), "x.tar.gz"), None);
        assert_eq!(parse_sidecar(&format!("{h}  x.tar.gz\n{h}  x.tar.gz\n"), "x.tar.gz"), None);
        assert_eq!(parse_sidecar("", "x.tar.gz"), None);
    }

    fn manifest(level: Option<i64>) -> Manifest {
        Manifest {
            format: FORMAT_NAME.to_string(),
            format_version: FORMAT_VERSION,
            instance_id: "0123456789abcdef".into(),
            mybibli_version: "1.30.0".into(),
            last_migration: level,
            created_at: "2026-11-26T08:00:00Z".into(),
            tables: BTreeMap::new(),
            covers: crate::services::backup::archive::CoverSummary { count: 0, bytes: 0, skipped: 0 },
            members: vec![],
        }
    }

    #[test]
    fn the_version_rule_refuses_newer_and_unknown_levels() {
        let known = |v: i64| v == 10 || v == 20;
        let check = |m: &Manifest| check_manifest(m, "0123456789abcdef", Some(20), known, "1.29.0");
        assert_eq!(check(&manifest(Some(20))), Ok(()));
        assert_eq!(check(&manifest(Some(10))), Ok(()), "an older known level");
        assert_eq!(
            check(&manifest(Some(21))),
            Err(Refusal::Newer { version: "1.30.0".into(), current: "1.29.0".into() })
        );
        assert_eq!(check(&manifest(Some(15))), Err(Refusal::NotABackup), "an unknown older level");
        assert_eq!(check(&manifest(None)), Err(Refusal::NotABackup));
        let mut m = manifest(Some(20));
        m.format_version = 2;
        assert_eq!(check(&m), Err(Refusal::NotABackup));
        let mut m = manifest(Some(20));
        m.format = "other".into();
        assert_eq!(check(&m), Err(Refusal::NotABackup));
        let mut m = manifest(Some(20));
        m.instance_id = "fedcba9876543210".into();
        assert_eq!(check(&m), Err(Refusal::NotABackup), "the id of the name");
    }

    #[test]
    fn the_open_refuses_links_fifos_and_unreadable_files_without_blocking() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("target"), b"x").unwrap();
        std::os::unix::fs::symlink(d.join("target"), d.join("link")).unwrap();
        assert_eq!(open_checked(&d.join("link"), "link").unwrap_err(), Refusal::Gone, "ELOOP");
        nix::unistd::mkfifo(&d.join("fifo"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        assert_eq!(open_checked(&d.join("fifo"), "fifo").unwrap_err(), Refusal::Gone, "a FIFO, opened without blocking");
        std::fs::create_dir(d.join("dir")).unwrap();
        assert_eq!(open_checked(&d.join("dir"), "dir").unwrap_err(), Refusal::Gone);
        assert_eq!(open_checked(&d.join("absent"), "absent").unwrap_err(), Refusal::Gone);
        assert!(open_checked(&d.join("target"), "target").is_ok());
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.join("target"), std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(d.join("target")).is_err() {
            assert_eq!(
                open_checked(&d.join("target"), "target").unwrap_err(),
                Refusal::NotReadable { file: "target".into() }
            );
        }
    }

    #[test]
    fn a_name_outside_the_shape_or_absent_is_gone() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(verify_backup(tmp.path(), "../etc/passwd").unwrap_err(), Refusal::Gone);
        assert_eq!(
            verify_backup(tmp.path(), "mybibli-0123456789abcdef-20261126T080000Z.tar.gz").unwrap_err(),
            Refusal::Gone
        );
        let dir_name = "mybibli-0123456789abcdef-20261126T090000Z.tar.gz";
        std::fs::create_dir(tmp.path().join(dir_name)).unwrap();
        assert_eq!(verify_backup(tmp.path(), dir_name).unwrap_err(), Refusal::Gone);
        let link = "mybibli-0123456789abcdef-20261126T100000Z.tar.gz";
        std::fs::write(tmp.path().join("target"), b"x").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("target"), tmp.path().join(link)).unwrap();
        assert_eq!(verify_backup(tmp.path(), link).unwrap_err(), Refusal::Gone, "a link");
        let no_sidecar = "mybibli-0123456789abcdef-20261126T110000Z.tar.gz";
        std::fs::write(tmp.path().join(no_sidecar), b"x").unwrap();
        assert_eq!(
            verify_backup(tmp.path(), no_sidecar).unwrap_err(),
            Refusal::NoSidecar { file: no_sidecar.into() }
        );
    }
}
