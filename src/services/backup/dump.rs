//! Story 19-6 (D2, D3) — `database.sql`, the SQL dump of a backup.
//!
//! A standard dump the `mariadb` client loads into an empty database:
//! a header (`SET NAMES utf8mb4`, UTC session time zone,
//! `NO_AUTO_VALUE_ON_ZERO`, foreign-key and unique checks off), then for
//! every base table of the schema — `_sqlx_migrations` included — its
//! `DROP TABLE IF EXISTS`, its `SHOW CREATE TABLE` text and multi-row
//! `INSERT`s of at most [`ROWS_PER_INSERT`] rows and about [`INSERT_BYTE_BUDGET`] bytes, in primary-key order;
//! the footer restores the checks and the SQL mode.
//!
//! Every row is copied, **Trash included** (`deleted_at IS NULL` does not
//! apply: a deliberate exception). Values are written as SQL literals
//! built **by the server** ([`literal_expr`]): `QUOTE()` for text,
//! a hex literal for binary and JSON columns, `CAST(… AS CHAR)` for
//! numbers — so every value survives a byte-exact round trip. Generated
//! columns are left out of the column lists (the server recomputes them).
//!
//! The rows are read in a `REPEATABLE READ` read-only transaction opened
//! `WITH CONSISTENT SNAPSHOT` (every table is InnoDB): readers and writers
//! go on as usual. The table definitions are read just before it starts.
//! Rows are streamed (`fetch`), so memory stays flat.

use std::collections::BTreeMap;
use std::io::Write;

use futures::TryStreamExt;
use sqlx::{Executor, MySqlConnection, Row};

use super::BackupError;

/// At most this many rows per `INSERT` statement.
pub const ROWS_PER_INSERT: usize = 500;

/// Review of story 19-6 (M2, decided by the project lead 2026-11-26) —
/// tables whose definition is dumped but not their rows. `sessions` holds
/// the session tokens in clear (the cookie value itself): a copy of the
/// file must never let its reader act as a signed-in user. Sessions are
/// ephemeral, and a restore logs everyone out anyway (#657).
pub const ROWLESS_TABLES: &[&str] = &["sessions"];

pub fn is_rowless(table: &str) -> bool {
    ROWLESS_TABLES.contains(&table)
}

/// Review of story 19-6 (pass 2, same decision as [`ROWLESS_TABLES`]) —
/// columns written as an empty string instead of their value: they hold a
/// session token in clear. `pending_metadata_updates.session_token` is the
/// cookie of the session that scanned the title; the rows themselves are
/// kept (the home page counts the failed ones), and a token matching no
/// session is already ignored by `middleware::pending_updates`.
pub const BLANKED_COLUMNS: &[(&str, &str)] = &[("pending_metadata_updates", "session_token")];

pub fn is_blanked(table: &str, column: &str) -> bool {
    BLANKED_COLUMNS.iter().any(|(t, c)| *t == table && *c == column)
}

/// How a column's values are written as SQL literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// `QUOTE(col)` (also gives the bare word `NULL`).
    Text,
    /// A hex literal (`0x…`), `''` when empty, `NULL`.
    Binary,
    /// `CAST(col AS CHAR)`, `NULL`.
    Number,
}

/// The kind of a column, from `information_schema.COLUMNS.DATA_TYPE` and
/// whether a `json_valid(col)` check marks it as a JSON column.
pub fn classify(data_type: &str, is_json: bool) -> ValueKind {
    if is_json {
        return ValueKind::Binary;
    }
    match data_type.to_ascii_lowercase().as_str() {
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "decimal"
        | "numeric" | "float" | "double" | "real" | "year" => ValueKind::Number,
        "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob" | "bit"
        | "json" => ValueKind::Binary,
        _ => ValueKind::Text,
    }
}

/// A backquoted identifier (backquotes doubled).
pub fn quote_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// The server-side expression that renders one column as an SQL literal.
/// Never NULL itself: a NULL value renders as the word `NULL`.
pub fn literal_expr(column: &str, kind: ValueKind) -> String {
    let c = quote_ident(column);
    match kind {
        ValueKind::Text => format!("QUOTE({c})"),
        ValueKind::Binary => format!(
            "IF({c} IS NULL, 'NULL', IF(LENGTH({c}) = 0, '''''', CONCAT('0x', HEX({c}))))"
        ),
        ValueKind::Number => format!("IFNULL(CAST({c} AS CHAR), 'NULL')"),
    }
}

/// One dumped column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSpec {
    pub name: String,
    pub kind: ValueKind,
}

/// One dumped table: its definition, its stored (non-generated) columns
/// and its primary key (row order).
#[derive(Debug, Clone)]
pub struct TableSpec {
    pub name: String,
    pub create_sql: String,
    pub columns: Vec<ColumnSpec>,
    pub primary_key: Vec<String>,
}

impl TableSpec {
    /// The `SELECT` that renders each row as its literals, one per column.
    pub fn select_sql(&self) -> String {
        let exprs: Vec<String> = self
            .columns
            .iter()
            .map(|c| {
                if is_blanked(&self.name, &c.name) {
                    // The SQL literal `''`, whatever the stored value.
                    "''''''".to_string()
                } else {
                    literal_expr(&c.name, c.kind)
                }
            })
            .collect();
        let mut sql = format!("SELECT {} FROM {}", exprs.join(", "), quote_ident(&self.name));
        if !self.primary_key.is_empty() {
            let keys: Vec<String> = self.primary_key.iter().map(|k| quote_ident(k)).collect();
            sql.push_str(" ORDER BY ");
            sql.push_str(&keys.join(", "));
        }
        sql
    }

    /// `INSERT INTO `t` (`a`, `b`) VALUES`
    pub fn insert_head(&self) -> String {
        let cols: Vec<String> = self.columns.iter().map(|c| quote_ident(&c.name)).collect();
        format!("INSERT INTO {} ({}) VALUES\n", quote_ident(&self.name), cols.join(", "))
    }
}

/// What the dump read.
#[derive(Debug, Clone, Default)]
pub struct DumpStats {
    /// Row count of each table.
    pub rows: BTreeMap<String, u64>,
    /// Highest `_sqlx_migrations.version`.
    pub last_migration: Option<i64>,
}

/// The dump header.
pub const HEADER: &str = "SET NAMES utf8mb4;\n\
SET time_zone = '+00:00';\n\
SET @OLD_SQL_MODE = @@SQL_MODE, SQL_MODE = 'NO_AUTO_VALUE_ON_ZERO';\n\
SET FOREIGN_KEY_CHECKS = 0;\n\
SET UNIQUE_CHECKS = 0;\n";

/// The dump footer.
pub const FOOTER: &str = "SET FOREIGN_KEY_CHECKS = 1;\n\
SET UNIQUE_CHECKS = 1;\n\
SET SQL_MODE = @OLD_SQL_MODE;\n";

/// Every base table of the current schema, with its definition and its
/// columns. Read outside the snapshot (DDL does not run while the app is
/// up).
pub async fn read_schema(conn: &mut MySqlConnection) -> Result<Vec<TableSpec>, BackupError> {
    let names: Vec<(String,)> = sqlx::query_as(
        "SELECT CAST(TABLE_NAME AS CHAR) FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE' ORDER BY TABLE_NAME",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut specs = Vec::with_capacity(names.len());
    for (name,) in names {
        let row = sqlx::query(&format!("SHOW CREATE TABLE {}", quote_ident(&name)))
            .fetch_one(&mut *conn)
            .await?;
        let create_sql: String = row.try_get(1)?;
        let json_checks: Vec<(String,)> = sqlx::query_as(
            "SELECT CAST(CHECK_CLAUSE AS CHAR) FROM information_schema.CHECK_CONSTRAINTS \
             WHERE CONSTRAINT_SCHEMA = DATABASE() AND TABLE_NAME = ?",
        )
        .bind(&name)
        .fetch_all(&mut *conn)
        .await?;
        let cols: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT CAST(COLUMN_NAME AS CHAR), CAST(DATA_TYPE AS CHAR), \
                    CAST(GENERATION_EXPRESSION IS NOT NULL OR EXTRA LIKE '%GENERATED%' AS SIGNED) \
             FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
        )
        .bind(&name)
        .fetch_all(&mut *conn)
        .await?;
        let columns = cols
            .into_iter()
            .filter(|(_, _, generated)| *generated == 0)
            .map(|(col, data_type, _)| {
                let json_check = format!("json_valid({})", quote_ident(&col));
                let is_json = json_checks.iter().any(|(c,)| c.trim() == json_check);
                ColumnSpec {
                    kind: classify(&data_type, is_json),
                    name: col,
                }
            })
            .collect();
        let pk: Vec<(String,)> = sqlx::query_as(
            "SELECT CAST(COLUMN_NAME AS CHAR) FROM information_schema.KEY_COLUMN_USAGE \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND CONSTRAINT_NAME = 'PRIMARY' \
             ORDER BY ORDINAL_POSITION",
        )
        .bind(&name)
        .fetch_all(&mut *conn)
        .await?;
        specs.push(TableSpec {
            name,
            create_sql,
            columns,
            primary_key: pk.into_iter().map(|(k,)| k).collect(),
        });
    }
    Ok(specs)
}

/// Open the read-only consistent snapshot on this connection (UTC session,
/// so `TIMESTAMP` values are written as stored). No lock is taken.
pub async fn begin_snapshot(conn: &mut MySqlConnection) -> Result<(), BackupError> {
    // Plain `&str` statements go through the text protocol (`START
    // TRANSACTION` cannot be prepared).
    for statement in [
        "SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "SET SESSION time_zone = '+00:00'",
        "START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY",
    ] {
        conn.execute(statement).await?;
    }
    Ok(())
}

/// Write the rows of every table, as of the open snapshot.
pub async fn dump_tables<W: Write>(
    conn: &mut MySqlConnection,
    specs: &[TableSpec],
    out: &mut W,
) -> Result<DumpStats, BackupError> {
    let mut stats = DumpStats::default();
    out.write_all(b"-- mybibli backup: database.sql\n")?;
    out.write_all(HEADER.as_bytes())?;
    for spec in specs {
        out.write_all(format!("\n-- Table {}\n", quote_ident(&spec.name)).as_bytes())?;
        out.write_all(format!("DROP TABLE IF EXISTS {};\n", quote_ident(&spec.name)).as_bytes())?;
        out.write_all(spec.create_sql.as_bytes())?;
        out.write_all(b";\n")?;
        let count = if spec.columns.is_empty() || is_rowless(&spec.name) {
            if is_rowless(&spec.name) {
                out.write_all(b"-- Rows not copied: session tokens are live credentials.\n")?;
            }
            0
        } else {
            dump_rows(conn, spec, out).await?
        };
        stats.rows.insert(spec.name.clone(), count);
    }
    out.write_all(b"\n")?;
    out.write_all(FOOTER.as_bytes())?;
    if specs.iter().any(|s| s.name == "_sqlx_migrations") {
        let (v,): (Option<i64>,) = sqlx::query_as("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&mut *conn)
            .await?;
        stats.last_migration = v;
    }
    Ok(stats)
}

async fn dump_rows<W: Write>(
    conn: &mut MySqlConnection,
    spec: &TableSpec,
    out: &mut W,
) -> Result<u64, BackupError> {
    let select = spec.select_sql();
    let mut rows = sqlx::query(&select).fetch(&mut *conn);
    let mut batch = InsertBatcher::new(spec.insert_head());
    let mut literals: Vec<Vec<u8>> = Vec::with_capacity(spec.columns.len());
    while let Some(row) = rows.try_next().await? {
        literals.clear();
        for i in 0..spec.columns.len() {
            // Review of story 19-6 (L1): every literal expression yields a
            // string, `'NULL'` included. A server-side NULL means the value
            // could not be rendered (e.g. above `max_allowed_packet`,
            // warning 1301): fail the backup rather than write a NULL.
            let literal: Option<Vec<u8>> = row.try_get(i)?;
            let Some(literal) = literal else {
                return Err(BackupError::Database(format!(
                    "a value of {}.{} could not be rendered as SQL",
                    spec.name, spec.columns[i].name
                )));
            };
            literals.push(literal);
        }
        batch.push_row(out, &literals)?;
    }
    batch.finish(out)
}

/// Review of story 19-6 (M1) — the most bytes an `INSERT` grows to before
/// the next row starts a new one (mysqldump's `net_buffer_length` order of
/// magnitude), well under the 16 MiB `max_allowed_packet` the `mariadb`
/// client and server default to. A single row larger than this is written
/// alone.
pub const INSERT_BYTE_BUDGET: usize = 1 << 20;

/// Multi-row `INSERT`s closed at [`ROWS_PER_INSERT`] rows **or** once they
/// reach [`INSERT_BYTE_BUDGET`] bytes, whichever comes first (pure: unit
/// tested on a fake row stream).
pub struct InsertBatcher {
    head: String,
    rows_in: usize,
    bytes_in: usize,
    total: u64,
}

impl InsertBatcher {
    pub fn new(head: String) -> Self {
        InsertBatcher {
            head,
            rows_in: 0,
            bytes_in: 0,
            total: 0,
        }
    }

    pub fn push_row<W: Write>(&mut self, out: &mut W, literals: &[Vec<u8>]) -> std::io::Result<()> {
        // Close the open statement first when this row would take it past
        // the budget (pass-2 review): a statement never holds more than
        // one row beyond the budget.
        // Exactly what a non-first row adds: `,\n`, the parentheses, the
        // literals and the commas between them.
        let row_len: usize = 2
            + 2
            + literals.iter().map(|l| l.len()).sum::<usize>()
            + literals.len().saturating_sub(1);
        if self.rows_in > 0 && self.bytes_in + row_len > INSERT_BYTE_BUDGET {
            out.write_all(b";\n")?;
            self.rows_in = 0;
            self.bytes_in = 0;
        }
        if self.rows_in == 0 {
            out.write_all(self.head.as_bytes())?;
            self.bytes_in = self.head.len();
        } else {
            out.write_all(b",\n")?;
            self.bytes_in += 2;
        }
        out.write_all(b"(")?;
        self.bytes_in += 2;
        for (i, literal) in literals.iter().enumerate() {
            if i > 0 {
                out.write_all(b",")?;
                self.bytes_in += 1;
            }
            out.write_all(literal)?;
            self.bytes_in += literal.len();
        }
        out.write_all(b")")?;
        self.rows_in += 1;
        self.total += 1;
        if self.rows_in == ROWS_PER_INSERT || self.bytes_in >= INSERT_BYTE_BUDGET {
            out.write_all(b";\n")?;
            self.rows_in = 0;
            self.bytes_in = 0;
        }
        Ok(())
    }

    /// Close the open statement, if any; the number of rows written.
    pub fn finish<W: Write>(self, out: &mut W) -> Result<u64, BackupError> {
        if self.rows_in > 0 {
            out.write_all(b";\n")?;
        }
        Ok(self.total)
    }
}

/// The whole dump: definitions, snapshot, rows, end of the transaction.
pub async fn write_dump<W: Write>(
    conn: &mut MySqlConnection,
    out: &mut W,
) -> Result<DumpStats, BackupError> {
    let specs = read_schema(conn).await?;
    begin_snapshot(conn).await?;
    let stats = dump_tables(conn, &specs, out).await?;
    conn.execute("COMMIT").await?;
    out.flush()?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_classified_by_type() {
        assert_eq!(classify("bigint", false), ValueKind::Number);
        assert_eq!(classify("DECIMAL", false), ValueKind::Number);
        assert_eq!(classify("tinyint", false), ValueKind::Number);
        assert_eq!(classify("blob", false), ValueKind::Binary);
        assert_eq!(classify("varbinary", false), ValueKind::Binary);
        assert_eq!(classify("longtext", true), ValueKind::Binary, "a JSON column");
        assert_eq!(classify("longtext", false), ValueKind::Text);
        assert_eq!(classify("varchar", false), ValueKind::Text);
        assert_eq!(classify("timestamp", false), ValueKind::Text);
        assert_eq!(classify("enum", false), ValueKind::Text);
        assert_eq!(classify("date", false), ValueKind::Text);
    }

    #[test]
    fn literals_are_built_by_the_server() {
        assert_eq!(literal_expr("name", ValueKind::Text), "QUOTE(`name`)");
        assert_eq!(
            literal_expr("n", ValueKind::Number),
            "IFNULL(CAST(`n` AS CHAR), 'NULL')"
        );
        assert_eq!(
            literal_expr("b", ValueKind::Binary),
            "IF(`b` IS NULL, 'NULL', IF(LENGTH(`b`) = 0, '''''', CONCAT('0x', HEX(`b`))))"
        );
    }

    #[test]
    fn identifiers_are_backquoted() {
        assert_eq!(quote_ident("titles"), "`titles`");
        assert_eq!(quote_ident("a`b"), "`a``b`");
    }

    #[test]
    fn the_pending_updates_session_token_is_blanked() {
        let spec = TableSpec {
            name: "pending_metadata_updates".to_string(),
            create_sql: String::new(),
            columns: vec![
                ColumnSpec { name: "id".to_string(), kind: ValueKind::Number },
                ColumnSpec { name: "session_token".to_string(), kind: ValueKind::Text },
            ],
            primary_key: vec!["id".to_string()],
        };
        let sql = spec.select_sql();
        assert!(!sql.contains("QUOTE(`session_token`)"), "{sql}");
        assert!(sql.contains("''''''"), "{sql}");
        assert!(is_blanked("pending_metadata_updates", "session_token"));
        assert!(!is_blanked("titles", "session_token"));
    }

    #[test]
    fn sessions_are_dumped_without_their_rows() {
        assert!(is_rowless("sessions"));
        assert!(!is_rowless("users"));
        assert!(!is_rowless("api_keys"));
    }

    #[test]
    fn inserts_close_at_the_row_count_or_the_byte_budget() {
        // Small rows: closed every ROWS_PER_INSERT rows.
        let mut out = Vec::new();
        let mut b = InsertBatcher::new("INSERT INTO `t` (`a`) VALUES\n".to_string());
        for _ in 0..(ROWS_PER_INSERT + 1) {
            b.push_row(&mut out, &[b"1".to_vec()]).unwrap();
        }
        assert_eq!(b.finish(&mut out).unwrap(), ROWS_PER_INSERT as u64 + 1);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("INSERT INTO").count(), 2);
        assert_eq!(text.matches(";\n").count(), 2);

        // Big rows: a statement is closed before a row would take it past the
        // budget, long before 500 rows (a quarter-budget row plus its framing:
        // three fit, a fourth would not).
        let big = vec![b'x'; INSERT_BYTE_BUDGET / 4];
        let mut out = Vec::new();
        let mut b = InsertBatcher::new("INSERT INTO `t` (`a`) VALUES\n".to_string());
        for _ in 0..10 {
            b.push_row(&mut out, std::slice::from_ref(&big)).unwrap();
        }
        assert_eq!(b.finish(&mut out).unwrap(), 10);
        let text = String::from_utf8(out).unwrap();
        let statements: Vec<&str> = text.split(";\n").filter(|s| !s.is_empty()).collect();
        assert_eq!(statements.len(), 4, "3 + 3 + 3 + 1 rows");
        for st in &statements {
            assert!(st.starts_with("INSERT INTO"), "{}", &st[..20]);
            assert!(st.len() <= INSERT_BYTE_BUDGET, "{}", st.len());
        }

        // A single row above the budget is written alone.
        let huge = vec![b'y'; INSERT_BYTE_BUDGET + 10];
        let mut out = Vec::new();
        let mut b = InsertBatcher::new("INSERT INTO `t` (`a`) VALUES\n".to_string());
        b.push_row(&mut out, std::slice::from_ref(&huge)).unwrap();
        b.push_row(&mut out, &[b"1".to_vec()]).unwrap();
        b.finish(&mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap().matches("INSERT INTO").count(), 2);
    }

    fn spec() -> TableSpec {
        TableSpec {
            name: "t".into(),
            create_sql: "CREATE TABLE `t` (…)".into(),
            columns: vec![
                ColumnSpec { name: "id".into(), kind: ValueKind::Number },
                ColumnSpec { name: "label".into(), kind: ValueKind::Text },
            ],
            primary_key: vec!["id".into()],
        }
    }

    #[test]
    fn the_select_and_insert_name_the_stored_columns_in_key_order() {
        let s = spec();
        assert_eq!(
            s.select_sql(),
            "SELECT IFNULL(CAST(`id` AS CHAR), 'NULL'), QUOTE(`label`) FROM `t` ORDER BY `id`"
        );
        assert_eq!(s.insert_head(), "INSERT INTO `t` (`id`, `label`) VALUES\n");
    }

    #[test]
    fn the_header_turns_checks_off_and_the_footer_back_on() {
        assert!(HEADER.starts_with("SET NAMES utf8mb4;"));
        assert!(HEADER.contains("FOREIGN_KEY_CHECKS = 0"));
        assert!(HEADER.contains("UNIQUE_CHECKS = 0"));
        assert!(FOOTER.contains("FOREIGN_KEY_CHECKS = 1"));
        assert!(FOOTER.contains("UNIQUE_CHECKS = 1"));
    }
}
