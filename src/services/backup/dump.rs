//! Story 19-6 (D2, D3) — `database.sql`, the SQL dump of a backup.
//!
//! A standard dump the `mariadb` client loads into an empty database:
//! a header (`SET NAMES utf8mb4`, UTC session time zone,
//! `NO_AUTO_VALUE_ON_ZERO`, foreign-key and unique checks off), then for
//! every base table of the schema — `_sqlx_migrations` included — its
//! `DROP TABLE IF EXISTS`, its `SHOW CREATE TABLE` text and multi-row
//! `INSERT`s of at most [`ROWS_PER_INSERT`] rows, in primary-key order;
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
        let exprs: Vec<String> =
            self.columns.iter().map(|c| literal_expr(&c.name, c.kind)).collect();
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
        let count = if spec.columns.is_empty() {
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
    let head = spec.insert_head();
    let select = spec.select_sql();
    let mut rows = sqlx::query(&select).fetch(&mut *conn);
    let mut count: u64 = 0;
    let mut in_batch = 0usize;
    while let Some(row) = rows.try_next().await? {
        if in_batch == 0 {
            out.write_all(head.as_bytes())?;
        } else {
            out.write_all(b",\n")?;
        }
        out.write_all(b"(")?;
        for i in 0..spec.columns.len() {
            if i > 0 {
                out.write_all(b",")?;
            }
            let literal: Option<Vec<u8>> = row.try_get(i)?;
            out.write_all(literal.as_deref().unwrap_or(b"NULL"))?;
        }
        out.write_all(b")")?;
        count += 1;
        in_batch += 1;
        if in_batch == ROWS_PER_INSERT {
            out.write_all(b";\n")?;
            in_batch = 0;
        }
    }
    if in_batch > 0 {
        out.write_all(b";\n")?;
    }
    Ok(count)
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
