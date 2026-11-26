//! Story 19-8 (#657, T5) — loading a backup into the live schema.
//!
//! On one dedicated connection (closed afterwards, never given back to the
//! pool): foreign-key checks off, **every** base table of the schema
//! dropped (a table a newer migration created would otherwise survive and
//! make the migrations fail), then `database.sql` executed statement by
//! statement, streamed from the archive (memory flat). The archive is read
//! in a blocking task that sends the statements through a small channel;
//! the same pass re-hashes every member against the verified manifest and
//! stages the covers as temporary files in `COVERS_DIR`
//! ([`super::covers`]). A member that differs from the manifest at this
//! point is a failure (the caller rolls back), never a silent continue.
//!
//! The splitter knows the dump's own syntax ([`StatementSplitter`]): a
//! statement ends with `;` at the end of a line outside single-quoted
//! strings (backslash escapes, `QUOTE()` output may hold raw newlines and
//! `;`), backquoted identifiers and double-quoted strings; `-- ` comment
//! lines between statements are skipped.
//!
//! Plain `&str` statements go through the text protocol (`conn.execute`);
//! `sqlx::raw_sql` in a spawned task fails the `Send` check (19-6).

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use sqlx::{Executor, MySqlConnection};

use super::covers::StagedCovers;
use super::verify::{HashingReader, Member, Verified, member_kind};
use crate::db::DbPool;
use crate::services::backup::archive::MemberEntry;

/// Why a load stopped. The message is for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError(pub String);

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<sqlx::Error> for LoadError {
    fn from(e: sqlx::Error) -> Self {
        LoadError(format!("database: {e}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Single,
    Double,
    Back,
}

/// Splits the dump into statements, line by line (pure, see module docs).
#[derive(Debug)]
pub struct StatementSplitter {
    buf: Vec<u8>,
    quote: Quote,
    escaped: bool,
}

impl Default for StatementSplitter {
    fn default() -> Self {
        StatementSplitter {
            buf: Vec::new(),
            quote: Quote::None,
            escaped: false,
        }
    }
}

impl StatementSplitter {
    /// Feed one line (with its `\n`, if any); returns the statement it
    /// completes, without its final `;`.
    pub fn push_line(&mut self, line: &[u8]) -> Option<Vec<u8>> {
        if self.quote == Quote::None && self.buf.iter().all(u8::is_ascii_whitespace) {
            let trimmed = line.trim_ascii_start();
            if trimmed.is_empty() || trimmed.starts_with(b"--") {
                return None;
            }
        }
        for &b in line {
            if self.escaped {
                self.escaped = false;
                continue;
            }
            match (self.quote, b) {
                (Quote::None, b'\'') => self.quote = Quote::Single,
                (Quote::None, b'"') => self.quote = Quote::Double,
                (Quote::None, b'`') => self.quote = Quote::Back,
                (Quote::Single, b'\\') | (Quote::Double, b'\\') => self.escaped = true,
                (Quote::Single, b'\'') | (Quote::Double, b'"') | (Quote::Back, b'`') => {
                    self.quote = Quote::None
                }
                _ => {}
            }
        }
        self.buf.extend_from_slice(line);
        if self.quote != Quote::None {
            return None;
        }
        let end = self.buf.trim_ascii_end();
        if end.last() != Some(&b';') {
            return None;
        }
        let statement = end[..end.len() - 1].to_vec();
        self.buf.clear();
        Some(statement)
    }

    /// Whatever is left at the end of the input (must be blank).
    pub fn rest(&self) -> &[u8] {
        self.buf.trim_ascii()
    }
}

/// Split a whole dump (tests, small inputs).
pub fn split_all(sql: &[u8]) -> Result<Vec<Vec<u8>>, LoadError> {
    let mut s = StatementSplitter::default();
    let mut out = Vec::new();
    for line in sql.split_inclusive(|b| *b == b'\n') {
        out.extend(s.push_line(line));
    }
    if !s.rest().is_empty() {
        return Err(LoadError("the dump ends inside a statement".to_string()));
    }
    Ok(out)
}

/// Drop every base table of the current schema (FK checks off on `conn`).
pub async fn drop_all_tables(conn: &mut MySqlConnection) -> Result<usize, LoadError> {
    conn.execute("SET FOREIGN_KEY_CHECKS = 0").await?;
    let names: Vec<(String,)> = sqlx::query_as(
        "SELECT CAST(TABLE_NAME AS CHAR) FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE'",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (name,) in &names {
        let statement = format!(
            "DROP TABLE IF EXISTS {}",
            crate::services::backup::dump::quote_ident(name)
        );
        conn.execute(statement.as_str()).await?;
    }
    Ok(names.len())
}

/// What the reading pass reports.
#[derive(Debug)]
pub struct Loaded {
    pub statements: u64,
    pub covers: StagedCovers,
}

/// Read the archive's members in order, checking each against `expected`
/// (the verified manifest's list), sending the dump's statements to `tx`
/// and staging the covers. Blocking.
fn read_archive(
    path: &Path,
    expected: &[MemberEntry],
    covers_dir: &Path,
    tx: tokio::sync::mpsc::Sender<String>,
) -> Result<StagedCovers, LoadError> {
    let io = |e: std::io::Error| LoadError(format!("reading the archive: {e}"));
    let file = std::fs::File::open(path).map_err(io)?;
    let mut archive = tar::Archive::new(GzDecoder::new(BufReader::with_capacity(1 << 16, file)));
    let mut staged = StagedCovers::new(covers_dir);
    let mut index = 0usize;
    for entry in archive.entries().map_err(io)? {
        let mut entry = entry.map_err(io)?;
        if entry.header().entry_type() != tar::EntryType::Regular {
            return Err(LoadError("an entry is not a regular file".to_string()));
        }
        let name = entry.path_bytes().into_owned();
        let kind = member_kind(&name).ok_or_else(|| LoadError("a member name is not of the backup shape".to_string()))?;
        if kind == Member::Manifest {
            if index != expected.len() {
                return Err(LoadError("the members changed since they were checked".to_string()));
            }
            index += 1;
            continue;
        }
        let want = expected
            .get(index)
            .filter(|m| m.name.as_bytes() == name.as_slice())
            .ok_or_else(|| LoadError("the members changed since they were checked".to_string()))?;
        index += 1;
        let (sha, size) = match kind {
            Member::Sql => {
                let mut hashing = HashingReader::new(&mut entry);
                {
                    let mut lines = BufReader::with_capacity(1 << 16, &mut hashing);
                    let mut splitter = StatementSplitter::default();
                    let mut line = Vec::new();
                    loop {
                        line.clear();
                        if lines.read_until(b'\n', &mut line).map_err(io)? == 0 {
                            break;
                        }
                        if let Some(statement) = splitter.push_line(&line) {
                            let statement = String::from_utf8(statement)
                                .map_err(|_| LoadError("a statement is not UTF-8".to_string()))?;
                            if tx.blocking_send(statement).is_err() {
                                return Err(LoadError("the load stopped".to_string()));
                            }
                        }
                    }
                    if !splitter.rest().is_empty() {
                        return Err(LoadError("the dump ends inside a statement".to_string()));
                    }
                }
                hashing.finish()
            }
            Member::Cover(file_name) => staged.stage(&file_name, &mut entry).map_err(io)?,
            Member::Manifest => unreachable!("handled above"),
        };
        if sha != want.sha256 || size != want.size {
            return Err(LoadError(format!(
                "member {} differs from the manifest checked before the restore",
                want.name
            )));
        }
    }
    if index != expected.len() + 1 {
        return Err(LoadError("the archive ended early".to_string()));
    }
    Ok(staged)
}

/// Drop every table, then load `verified`'s dump from `archive` on a
/// dedicated connection; the covers are staged (not yet in place). On any
/// failure the staged covers are removed and the database is left as the
/// load left it (the caller rolls back).
pub async fn load(
    pool: &DbPool,
    archive: PathBuf,
    verified: &Verified,
    covers_dir: &Path,
) -> Result<Loaded, LoadError> {
    let mut conn = pool.acquire().await?;
    // Session settings change (FK checks, SQL mode): never back to the pool.
    conn.close_on_drop();
    conn.execute("SET time_zone = '+00:00'").await?;
    let dropped = drop_all_tables(&mut conn).await?;
    tracing::info!(tables = dropped, file = %verified.file_name, "Restore: every table dropped; loading the backup");

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(4);
    let expected = verified.manifest.members.clone();
    let covers = covers_dir.to_path_buf();
    let reader = tokio::task::spawn_blocking(move || read_archive(&archive, &expected, &covers, tx));
    let mut statements = 0u64;
    let mut failed: Option<LoadError> = None;
    while let Some(statement) = rx.recv().await {
        if let Err(e) = conn.execute(statement.as_str()).await {
            failed = Some(LoadError(format!("statement {} failed: {e}", statements + 1)));
            break;
        }
        statements += 1;
    }
    // Closing the channel stops the reader at its next statement.
    drop(rx);
    let read = reader
        .await
        .map_err(|e| LoadError(format!("archive task ended abnormally: {e}")))?;
    let covers = match (read, failed) {
        (Ok(staged), None) => staged,
        (Ok(staged), Some(e)) => {
            drop(staged);
            return Err(e);
        }
        (Err(_), Some(e)) => return Err(e),
        (Err(e), None) => return Err(e),
    };
    // The dump's footer restored the checks; make sure for what follows.
    conn.execute("SET FOREIGN_KEY_CHECKS = 1").await?;
    drop(conn);
    Ok(Loaded { statements, covers })
}

/// D12 / T5: the local `instance_id` replaces the backup's (inserted when
/// the backup had none).
pub async fn keep_instance_id(pool: &DbPool, instance_id: &str) -> Result<(), LoadError> {
    sqlx::query(
        "INSERT INTO settings (setting_key, setting_value) VALUES (?, ?) \
         ON DUPLICATE KEY UPDATE setting_value = VALUES(setting_value), deleted_at = NULL",
    )
    .bind(crate::services::instance_id::SETTING_KEY)
    .bind(instance_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Read a whole small archive member into memory (tests and tools).
pub fn read_member(path: &Path, member: &str) -> std::io::Result<Option<Vec<u8>>> {
    let file = std::fs::File::open(path)?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.path_bytes().as_ref() == member.as_bytes() {
            let mut out = Vec::new();
            entry.read_to_end(&mut out)?;
            return Ok(Some(out));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::backup::dump::{FOOTER, HEADER, InsertBatcher};

    fn strings(v: Vec<Vec<u8>>) -> Vec<String> {
        v.into_iter().map(|s| String::from_utf8(s).unwrap()).collect()
    }

    #[test]
    fn statements_end_with_a_semicolon_at_the_end_of_a_line() {
        let sql = format!(
            "-- mybibli backup: database.sql\n{HEADER}\n-- Table `t`\nDROP TABLE IF EXISTS `t`;\n\
             CREATE TABLE `t` (\n  `a` varchar(10) DEFAULT 'x;y' COMMENT 'it''s; fine',\n  `b;` int\n);\n{FOOTER}"
        );
        let got = strings(split_all(sql.as_bytes()).unwrap());
        assert_eq!(got[0], "SET NAMES utf8mb4");
        assert_eq!(got[5], "DROP TABLE IF EXISTS `t`");
        assert!(got[6].starts_with("CREATE TABLE `t` (\n"), "{}", got[6]);
        assert!(got[6].ends_with("`b;` int\n)"), "{}", got[6]);
        assert_eq!(got.len(), 5 + 2 + 3);
        assert_eq!(got.last().unwrap(), "SET SQL_MODE = @OLD_SQL_MODE");
    }

    #[test]
    fn quoted_values_may_hold_newlines_semicolons_and_escapes() {
        // What the writer produces: QUOTE() output with raw newlines, `;`
        // at the end of a line, escaped quotes and backslashes.
        let mut out = Vec::new();
        let mut batch = InsertBatcher::new("INSERT INTO `t` (`a`, `b`) VALUES\n".to_string());
        batch
            .push_row(&mut out, &[b"'line 1;\nline 2 \\';\n\\\\'".to_vec(), b"NULL".to_vec()])
            .unwrap();
        batch
            .push_row(&mut out, &[b"'-- not a comment;\n-- still in the value'".to_vec(), b"0x3B0A".to_vec()])
            .unwrap();
        batch.finish(&mut out).unwrap();
        out.extend_from_slice(b"INSERT INTO `t` (`a`) VALUES\n('\"q\";\n`b`;\n');\n");
        let got = strings(split_all(&out).unwrap());
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].contains("line 1;\nline 2 \\';\n\\\\'"), "{}", got[0]);
        assert!(got[0].contains("'-- not a comment;\n-- still in the value'"));
        assert!(got[1].ends_with("('\"q\";\n`b`;\n')"));
    }

    #[test]
    fn comment_lines_between_statements_are_skipped_and_a_cut_dump_is_refused() {
        let got = strings(split_all(b"-- a 'quote in a comment\n\nSELECT 1;\n-- Rows not copied.\n").unwrap());
        assert_eq!(got, vec!["SELECT 1"]);
        assert!(split_all(b"INSERT INTO `t` VALUES ('open;\n").is_err());
        assert!(split_all(b"SELECT 1\n").is_err(), "no final semicolon");
    }
}
