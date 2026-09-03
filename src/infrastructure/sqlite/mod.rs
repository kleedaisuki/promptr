//! SQLite 持久化适配器。 / SQLite persistence adapter.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::{
    application::{
        Value,
        ports::{CatalogRead, CatalogWrite, Database},
    },
    diagnostic::{Diagnostic, DiagnosticCategory, Result},
    domain::{
        CatalogSnapshot, Metadata, Node, NodeBody, NodeHeader, NodeId, NodeKind, NonEmptyChildren,
        Revision, Symbol, Tag, XmlText,
    },
};

/// @brief Promptr SQLite 文件标识。 / Promptr SQLite application identifier.
pub const APPLICATION_ID: i32 = 0x5052_4d50;
/// @brief 当前可读写的数据库版本。 / Current readable and writable database version.
pub const SCHEMA_VERSION: i32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_millis(2_000);
const MIGRATION_NAME: &str = "0001_initial";

const SCHEMA_SQL: &str = r#"
CREATE TABLE schema_migrations(
  version INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, checksum TEXT NOT NULL,
  applied_at_ms INTEGER NOT NULL, app_version TEXT NOT NULL
) STRICT;
CREATE TABLE nodes(
  id INTEGER PRIMARY KEY, symbol TEXT NOT NULL UNIQUE, kind TEXT NOT NULL CHECK(kind IN ('fragment','prompt')),
  revision INTEGER NOT NULL CHECK(revision > 0), description TEXT,
  created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE fragments(
  node_id INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE, text TEXT NOT NULL
) STRICT;
CREATE TABLE prompt_edges(
  parent_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  position INTEGER NOT NULL CHECK(position >= 0), child_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE RESTRICT,
  PRIMARY KEY(parent_id, position)
) STRICT;
CREATE INDEX prompt_edges_child ON prompt_edges(child_id, parent_id, position);
CREATE TABLE tags(tag TEXT PRIMARY KEY) STRICT;
CREATE TABLE node_tags(
  node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
  tag TEXT NOT NULL REFERENCES tags(tag) ON DELETE RESTRICT,
  PRIMARY KEY(node_id, tag)
) STRICT;
CREATE TABLE app_state(key TEXT PRIMARY KEY, value INTEGER NOT NULL) STRICT;
INSERT INTO app_state(key,value) VALUES('catalog_revision',0);
"#;

const FTS_SQL: &str = r#"
CREATE VIRTUAL TABLE node_fts USING fts5(node_id UNINDEXED, symbol, content, description, tags);
"#;

/// @brief 数据库头与迁移状态。 / Database header and migration status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseStatus {
    /// @brief 数据库路径，内存库为空。 / Database path, absent for an in-memory store.
    pub path: Option<PathBuf>,
    /// @brief SQLite application_id。 / SQLite application_id.
    pub application_id: i32,
    /// @brief SQLite user_version。 / SQLite user_version.
    pub user_version: i32,
    /// @brief 迁移台账最高版本。 / Highest migration-ledger version.
    pub ledger_version: Option<i32>,
    /// @brief 当前程序能否正常解释该库。 / Whether this executable can interpret the database normally.
    pub compatible: bool,
}

/// @brief 数据库一致性检查结果。 / Database consistency-check result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// @brief integrity_check/quick_check 的原始行。 / Raw integrity-check rows.
    pub integrity: Vec<String>,
    /// @brief foreign_key_check 返回的违规数。 / Number of foreign-key violations.
    pub foreign_key_violations: usize,
    /// @brief 规范表能否恢复为有效领域快照。 / Whether canonical tables rehydrate a valid domain snapshot.
    pub domain_valid: bool,
}

/// @brief 同步 SQLite 数据库适配器。 / Synchronous SQLite database adapter.
pub struct SqliteDatabase {
    connection: Mutex<Connection>,
    path: Option<PathBuf>,
    normal_error: Option<Diagnostic>,
}

impl SqliteDatabase {
    /// @brief 打开或创建本地 SQLite 数据库。 / Open or create a local SQLite database.
    /// @param path 数据库文件路径。 / Database file path.
    /// @return 已配置的适配器或诊断。 / Configured adapter or diagnostic.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let connection = Connection::open(&path).map_err(storage)?;
        Self::finish_open(connection, Some(path), false)
    }

    /// @brief 打开独立内存数据库。 / Open an isolated in-memory database.
    /// @return 已配置的适配器或诊断。 / Configured adapter or diagnostic.
    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory().map_err(storage)?;
        Self::finish_open(connection, None, true)
    }

    fn finish_open(
        mut connection: Connection,
        path: Option<PathBuf>,
        memory: bool,
    ) -> Result<Self> {
        configure(&connection, memory)?;
        let application_id: i32 = connection
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(storage)?;
        let user_version: i32 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(storage)?;
        if application_id != 0 && application_id != APPLICATION_ID {
            return Err(diag(
                "E_DB_ID",
                DiagnosticCategory::Compatibility,
                format!(
                    "database application_id {application_id} does not identify a Promptr database"
                ),
            ));
        }
        if application_id == 0 && user_version == 0 {
            initialize(&mut connection)?;
        }
        let found: i32 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(storage)?;
        let normal_error = inspect_normal_error(&connection, found)?;
        Ok(Self {
            connection: Mutex::new(connection),
            path,
            normal_error,
        })
    }

    /// @brief 读取数据库版本和身份。 / Read database version and identity.
    /// @return 即使是更新版本数据库也可用的状态。 / Status available even for a newer database.
    pub fn status(&self) -> Result<DatabaseStatus> {
        let conn = self.lock()?;
        let application_id = pragma_i32(&conn, "application_id")?;
        let user_version = pragma_i32(&conn, "user_version")?;
        let ledger_version = if table_exists(&conn, "schema_migrations")? {
            conn.query_row("SELECT max(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .map_err(storage)?
        } else {
            None
        };
        Ok(DatabaseStatus {
            path: self.path.clone(),
            application_id,
            user_version,
            ledger_version,
            compatible: application_id == APPLICATION_ID && self.normal_error.is_none(),
        })
    }

    /// @brief 读取跨连接的目录修订号。 / Read the cross-connection catalog revision.
    /// @return 单调目录修订号或诊断。 / Monotonic catalog revision or diagnostic.
    pub fn catalog_revision(&self) -> Result<u64> {
        self.ensure_compatible()?;
        let conn = self.lock()?;
        let raw: i64 = conn
            .query_row(
                "SELECT value FROM app_state WHERE key='catalog_revision'",
                [],
                |row| row.get(0),
            )
            .map_err(storage)?;
        u64::try_from(raw).map_err(|_| {
            diag(
                "E_DB_CATALOG_REVISION",
                DiagnosticCategory::Internal,
                "persisted catalog revision is negative",
            )
        })
    }

    /// @brief 读取 SQLite data_version 以检测外部提交。 / Read SQLite data_version to detect external commits.
    /// @return 当前连接观察到的版本或诊断。 / Version observed by this connection or diagnostic.
    pub fn data_version(&self) -> Result<u64> {
        let conn = self.lock()?;
        let raw: i64 = conn
            .pragma_query_value(None, "data_version", |row| row.get(0))
            .map_err(storage)?;
        u64::try_from(raw).map_err(|_| {
            diag(
                "E_DB_DATA_VERSION",
                DiagnosticCategory::Internal,
                "SQLite returned a negative data_version",
            )
        })
    }

    /// @brief 运行 SQLite、外键与领域一致性检查。 / Run SQLite, foreign-key, and domain consistency checks.
    /// @param full 是否使用完整 integrity_check。 / Whether to use the full integrity_check.
    /// @return 检查报告或诊断。 / Check report or diagnostic.
    pub fn check(&self, full: bool) -> Result<CheckReport> {
        self.ensure_compatible()?;
        let conn = self.lock()?;
        let pragma = if full {
            "PRAGMA integrity_check"
        } else {
            "PRAGMA quick_check"
        };
        let mut statement = conn.prepare(pragma).map_err(storage)?;
        let integrity = statement
            .query_map([], |r| r.get(0))
            .map_err(storage)?
            .collect::<std::result::Result<Vec<String>, _>>()
            .map_err(storage)?;
        let violations = conn
            .prepare("PRAGMA foreign_key_check")
            .map_err(storage)?
            .query_map([], |_| Ok(()))
            .map_err(storage)?
            .count();
        let domain_valid = load_snapshot(&conn).is_ok();
        Ok(CheckReport {
            integrity,
            foreign_key_violations: violations,
            domain_valid,
        })
    }

    /// @brief 使用 SQLite Online Backup API 创建一致备份。 / Create a consistent backup with SQLite Online Backup API.
    /// @param destination 目标数据库文件。 / Destination database file.
    /// @return 成功或诊断。 / Success or diagnostic.
    pub fn backup(&self, destination: impl AsRef<Path>) -> Result<()> {
        let conn = self.lock()?;
        conn.backup("main", destination, None).map_err(storage)
    }

    /// @brief 从规范表重建派生 FTS5 索引。 / Rebuild the derived FTS5 index from canonical tables.
    /// @return 成功或诊断。 / Success or diagnostic.
    pub fn rebuild_index(&mut self) -> Result<()> {
        self.ensure_compatible()?;
        let conn = self.connection.get_mut().map_err(|_| poisoned())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        rebuild_fts(&tx)?;
        tx.commit().map_err(storage)
    }

    /// @brief 修复可安全重建的版本镜像和 FTS 索引。 / Repair the safely reconstructible version mirror and FTS index.
    /// @return 成功或诊断。 / Success or diagnostic.
    pub fn repair_derived(&mut self) -> Result<()> {
        let conn = self.connection.get_mut().map_err(|_| poisoned())?;
        let header_version = pragma_i32(conn, "user_version")?;
        if header_version > SCHEMA_VERSION {
            return Err(schema_new(header_version));
        }
        let ledger: Option<i32> = conn
            .query_row("SELECT max(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .map_err(storage)?;
        let ledger = ledger.ok_or_else(|| {
            diag(
                "E_DB_MIGRATION",
                DiagnosticCategory::Internal,
                "migration ledger is empty",
            )
        })?;
        if ledger > SCHEMA_VERSION {
            return Err(schema_new(ledger));
        }
        if ledger == 1 {
            let (name, checksum): (String, String) = conn
                .query_row(
                    "SELECT name,checksum FROM schema_migrations WHERE version=1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(storage)?;
            let expected = format!("{:x}", Sha256::digest(SCHEMA_SQL.as_bytes()));
            if name != MIGRATION_NAME || checksum != expected {
                return Err(diag(
                    "E_DB_MIGRATION",
                    DiagnosticCategory::Internal,
                    "migration 1 metadata or checksum does not match this executable",
                ));
            }
        }
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        tx.pragma_update(None, "user_version", ledger)
            .map_err(storage)?;
        rebuild_fts(&tx)?;
        tx.commit().map_err(storage)?;
        self.normal_error = inspect_normal_error(conn, ledger)?;
        if let Some(error) = self.normal_error.clone() {
            return Err(error);
        }
        Ok(())
    }

    fn ensure_compatible(&self) -> Result<()> {
        self.normal_error.clone().map_or(Ok(()), Err)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| poisoned())
    }
}

impl CatalogRead for SqliteDatabase {
    fn snapshot(&self) -> Result<CatalogSnapshot> {
        self.ensure_compatible()?;
        let conn = self.lock()?;
        let tx = conn.unchecked_transaction().map_err(storage)?;
        let snapshot = load_snapshot(&tx)?;
        tx.commit().map_err(storage)?;
        Ok(snapshot)
    }
}

impl Database for SqliteDatabase {
    fn write_transaction(
        &mut self,
        operation: &mut dyn FnMut(&mut dyn CatalogWrite) -> Result<Vec<Value>>,
    ) -> Result<Vec<Value>> {
        self.ensure_compatible()?;
        let conn = self.connection.get_mut().map_err(|_| poisoned())?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let mut writer = SqliteCatalogWrite { transaction: tx };
        let values = operation(&mut writer)?;
        load_snapshot(&writer.transaction)?;
        rebuild_fts(&writer.transaction)?;
        writer.transaction.commit().map_err(storage)?;
        Ok(values)
    }
}

struct SqliteCatalogWrite<'connection> {
    transaction: Transaction<'connection>,
}

impl CatalogRead for SqliteCatalogWrite<'_> {
    fn snapshot(&self) -> Result<CatalogSnapshot> {
        load_snapshot(&self.transaction)
    }
}

impl CatalogWrite for SqliteCatalogWrite<'_> {
    fn upsert_fragment(
        &mut self,
        target: &Symbol,
        text: &XmlText,
        expected_revision: Option<Revision>,
    ) -> Result<()> {
        let now = now_ms();
        let existing = find_node(&self.transaction, target)?;
        match existing {
            None => {
                if let Some(expected) = expected_revision {
                    return Err(conflict(target, Some(expected.get()), None));
                }
                self.transaction.execute("INSERT INTO nodes(symbol,kind,revision,created_at_ms,updated_at_ms) VALUES(?1,'fragment',1,?2,?2)", params![target.as_str(), now]).map_err(storage)?;
                let id = self.transaction.last_insert_rowid();
                self.transaction
                    .execute(
                        "INSERT INTO fragments(node_id,text) VALUES(?1,?2)",
                        params![id, text.as_str()],
                    )
                    .map_err(storage)?;
            }
            Some((id, kind, revision)) => {
                if kind != "fragment" {
                    return Err(diag(
                        "E_NODE_KIND",
                        DiagnosticCategory::Domain,
                        format!("symbol `{target}` is a prompt, not a fragment"),
                    ));
                }
                if expected_revision.is_some_and(|r| r.get() != revision) {
                    return Err(conflict(
                        target,
                        expected_revision.map(Revision::get),
                        Some(revision),
                    ));
                }
                let next = revision.checked_add(1).ok_or_else(revision_overflow)?;
                let next = i64::try_from(next).map_err(|_| revision_overflow())?;
                self.transaction
                    .execute(
                        "UPDATE nodes SET revision=?2,updated_at_ms=?3 WHERE id=?1",
                        params![id, next, now],
                    )
                    .map_err(storage)?;
                self.transaction
                    .execute(
                        "UPDATE fragments SET text=?2 WHERE node_id=?1",
                        params![id, text.as_str()],
                    )
                    .map_err(storage)?;
            }
        }
        bump_catalog(&self.transaction)
    }

    fn replace_prompt(&mut self, target: &Symbol, children: &[Symbol]) -> Result<()> {
        if children.is_empty() {
            return Err(domain(crate::domain::DomainError::EmptyChildren));
        }
        let mut child_ids = Vec::with_capacity(children.len());
        for child in children {
            let Some((id, _, _)) = find_node(&self.transaction, child)? else {
                return Err(diag(
                    "E_REF_MISSING",
                    DiagnosticCategory::ReferentialIntegrity,
                    format!("referenced symbol `{child}` does not exist"),
                ));
            };
            child_ids.push(id);
        }
        let now = now_ms();
        let parent_id = match find_node(&self.transaction, target)? {
            None => {
                self.transaction.execute("INSERT INTO nodes(symbol,kind,revision,created_at_ms,updated_at_ms) VALUES(?1,'prompt',1,?2,?2)", params![target.as_str(),now]).map_err(storage)?;
                self.transaction.last_insert_rowid()
            }
            Some((id, "prompt", revision)) => {
                let next = revision
                    .checked_add(1)
                    .and_then(|v| i64::try_from(v).ok())
                    .ok_or_else(revision_overflow)?;
                self.transaction
                    .execute(
                        "UPDATE nodes SET revision=?2,updated_at_ms=?3 WHERE id=?1",
                        params![id, next, now],
                    )
                    .map_err(storage)?;
                self.transaction
                    .execute("DELETE FROM prompt_edges WHERE parent_id=?1", [id])
                    .map_err(storage)?;
                id
            }
            Some(_) => {
                return Err(diag(
                    "E_NODE_KIND",
                    DiagnosticCategory::Domain,
                    format!("symbol `{target}` is a fragment, not a prompt"),
                ));
            }
        };
        for (position, child_id) in child_ids.into_iter().enumerate() {
            self.transaction
                .execute(
                    "INSERT INTO prompt_edges(parent_id,position,child_id) VALUES(?1,?2,?3)",
                    params![parent_id, position as i64, child_id],
                )
                .map_err(storage)?;
        }
        bump_catalog(&self.transaction)
    }

    fn rename(&mut self, target: &Symbol, new_symbol: &Symbol) -> Result<()> {
        let Some((id, _, revision)) = find_node(&self.transaction, target)? else {
            return Err(missing(target));
        };
        let next = revision
            .checked_add(1)
            .and_then(|v| i64::try_from(v).ok())
            .ok_or_else(revision_overflow)?;
        self.transaction
            .execute(
                "UPDATE nodes SET symbol=?2,revision=?3,updated_at_ms=?4 WHERE id=?1",
                params![id, new_symbol.as_str(), next, now_ms()],
            )
            .map_err(|e| {
                if is_constraint(&e) {
                    diag(
                        "E_SYMBOL_EXISTS",
                        DiagnosticCategory::Conflict,
                        format!("symbol `{new_symbol}` already exists"),
                    )
                } else {
                    storage(e)
                }
            })?;
        bump_catalog(&self.transaction)
    }

    fn delete(&mut self, target: &Symbol) -> Result<()> {
        let Some((id, _, _)) = find_node(&self.transaction, target)? else {
            return Err(missing(target));
        };
        let refs: i64 = self
            .transaction
            .query_row(
                "SELECT count(*) FROM prompt_edges WHERE child_id=?1",
                [id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if refs != 0 {
            return Err(diag(
                "E_NODE_REFERENCED",
                DiagnosticCategory::ReferentialIntegrity,
                format!("symbol `{target}` is referenced {refs} time(s)"),
            ));
        }
        self.transaction
            .execute("DELETE FROM nodes WHERE id=?1", [id])
            .map_err(storage)?;
        self.transaction.execute("DELETE FROM tags WHERE NOT EXISTS(SELECT 1 FROM node_tags WHERE node_tags.tag=tags.tag)",[]).map_err(storage)?;
        bump_catalog(&self.transaction)
    }

    fn set_metadata(&mut self, target: &Symbol, metadata: &Metadata) -> Result<()> {
        let Some((id, _, revision)) = find_node(&self.transaction, target)? else {
            return Err(missing(target));
        };
        let next = revision
            .checked_add(1)
            .and_then(|v| i64::try_from(v).ok())
            .ok_or_else(revision_overflow)?;
        self.transaction
            .execute(
                "UPDATE nodes SET description=?2,revision=?3,updated_at_ms=?4 WHERE id=?1",
                params![id, metadata.description(), next, now_ms()],
            )
            .map_err(storage)?;
        self.transaction
            .execute("DELETE FROM node_tags WHERE node_id=?1", [id])
            .map_err(storage)?;
        for tag in metadata.tags() {
            self.transaction
                .execute(
                    "INSERT INTO tags(tag) VALUES(?1) ON CONFLICT DO NOTHING",
                    [tag.as_str()],
                )
                .map_err(storage)?;
            self.transaction
                .execute(
                    "INSERT INTO node_tags(node_id,tag) VALUES(?1,?2)",
                    params![id, tag.as_str()],
                )
                .map_err(storage)?;
        }
        self.transaction.execute("DELETE FROM tags WHERE NOT EXISTS(SELECT 1 FROM node_tags WHERE node_tags.tag=tags.tag)",[]).map_err(storage)?;
        bump_catalog(&self.transaction)
    }
}

fn initialize(connection: &mut Connection) -> Result<()> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage)?;
    tx.execute_batch(SCHEMA_SQL).map_err(storage)?;
    tx.execute_batch(FTS_SQL).map_err(storage)?;
    let checksum = format!("{:x}", Sha256::digest(SCHEMA_SQL.as_bytes()));
    tx.execute("INSERT INTO schema_migrations(version,name,checksum,applied_at_ms,app_version) VALUES(1,?1,?2,?3,?4)",params![MIGRATION_NAME,checksum,now_ms(),env!("CARGO_PKG_VERSION")]).map_err(storage)?;
    tx.pragma_update(None, "application_id", APPLICATION_ID)
        .map_err(storage)?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(storage)?;
    tx.commit().map_err(storage)
}

fn inspect_normal_error(connection: &Connection, user_version: i32) -> Result<Option<Diagnostic>> {
    if user_version > SCHEMA_VERSION {
        return Ok(Some(schema_new(user_version)));
    }
    if !table_exists(connection, "schema_migrations")? {
        return Ok(Some(diag(
            "E_DB_MIGRATION",
            DiagnosticCategory::Internal,
            "migration ledger is missing",
        )));
    }
    let ledger: Option<(i32, String, String)> = connection
        .query_row(
            "SELECT version,name,checksum FROM schema_migrations ORDER BY version DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage)?;
    let Some((ledger_version, name, checksum)) = ledger else {
        return Ok(Some(diag(
            "E_DB_MIGRATION",
            DiagnosticCategory::Internal,
            "migration ledger is empty",
        )));
    };
    if ledger_version != user_version {
        return Ok(Some(diag(
            "E_DB_VERSION_MIRROR",
            DiagnosticCategory::Internal,
            format!("user_version {user_version} differs from migration ledger {ledger_version}"),
        )));
    }
    if ledger_version == 1 {
        let expected = format!("{:x}", Sha256::digest(SCHEMA_SQL.as_bytes()));
        if name != MIGRATION_NAME || checksum != expected {
            return Ok(Some(diag(
                "E_DB_MIGRATION",
                DiagnosticCategory::Internal,
                "migration 1 metadata or checksum does not match this executable",
            )));
        }
    }
    Ok(None)
}

fn configure(connection: &Connection, memory: bool) -> Result<()> {
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(storage)?;
    if !memory {
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(storage)?;
    }
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(storage)?;
    connection.busy_timeout(BUSY_TIMEOUT).map_err(storage)?;
    connection
        .pragma_update(None, "trusted_schema", false)
        .map_err(storage)?;
    let foreign: i32 = pragma_i32(connection, "foreign_keys")?;
    let sync: i32 = pragma_i32(connection, "synchronous")?;
    let trusted: i32 = pragma_i32(connection, "trusted_schema")?;
    let busy_timeout: i32 = pragma_i32(connection, "busy_timeout")?;
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .map_err(storage)?;
    if foreign != 1
        || sync != 2
        || trusted != 0
        || busy_timeout != i32::try_from(BUSY_TIMEOUT.as_millis()).unwrap_or(i32::MAX)
        || (!memory && !journal.eq_ignore_ascii_case("wal"))
    {
        return Err(diag(
            "E_DB_PRAGMA",
            DiagnosticCategory::Storage,
            "SQLite connection policy could not be established",
        ));
    }
    Ok(())
}

fn load_snapshot(connection: &Connection) -> Result<CatalogSnapshot> {
    let mut statement=connection.prepare("SELECT id,symbol,kind,revision,description,created_at_ms,updated_at_ms FROM nodes ORDER BY id").map_err(storage)?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })
        .map_err(storage)?;
    let mut nodes = Vec::new();
    for row in rows {
        let (raw_id, raw_symbol, kind, raw_revision, description, created_at_ms, updated_at_ms) =
            row.map_err(storage)?;
        let id = NodeId::new(raw_id).map_err(domain)?;
        let symbol = Symbol::new(raw_symbol).map_err(domain)?;
        let revision = Revision::new(u64::try_from(raw_revision).map_err(|_| {
            domain(crate::domain::DomainError::InvalidPositiveInteger {
                kind: "revision",
                value: raw_revision as i128,
            })
        })?)
        .map_err(domain)?;
        let tags = load_tags(connection, raw_id)?;
        let metadata = Metadata::new(description, tags);
        let (node_kind, body) = match kind.as_str() {
            "fragment" => {
                let text: String = connection
                    .query_row(
                        "SELECT text FROM fragments WHERE node_id=?1",
                        [raw_id],
                        |r| r.get(0),
                    )
                    .map_err(storage)?;
                (
                    NodeKind::Fragment,
                    NodeBody::Fragment(XmlText::new(text).map_err(domain)?),
                )
            }
            "prompt" => {
                let mut s = connection
                    .prepare(
                        "SELECT child_id FROM prompt_edges WHERE parent_id=?1 ORDER BY position",
                    )
                    .map_err(storage)?;
                let ids = s
                    .query_map([raw_id], |r| r.get::<_, i64>(0))
                    .map_err(storage)?
                    .map(|x| {
                        x.map_err(storage)
                            .and_then(|v| NodeId::new(v).map_err(domain))
                    })
                    .collect::<Result<Vec<_>>>()?;
                (
                    NodeKind::Prompt,
                    NodeBody::Prompt(NonEmptyChildren::new(ids).map_err(domain)?),
                )
            }
            other => {
                return Err(diag(
                    "E_DB_KIND",
                    DiagnosticCategory::Internal,
                    format!("unknown persisted node kind `{other}`"),
                ));
            }
        };
        nodes.push(
            Node::from_parts(
                NodeHeader {
                    id,
                    symbol,
                    kind: node_kind,
                    revision,
                    metadata,
                    created_at_ms,
                    updated_at_ms,
                },
                body,
            )
            .map_err(domain)?,
        );
    }
    CatalogSnapshot::new(nodes).map_err(domain)
}

fn load_tags(connection: &Connection, node_id: i64) -> Result<Vec<Tag>> {
    let mut statement = connection
        .prepare("SELECT tag FROM node_tags WHERE node_id=?1 ORDER BY tag")
        .map_err(storage)?;
    statement
        .query_map([node_id], |r| r.get::<_, String>(0))
        .map_err(storage)?
        .map(|row| {
            row.map_err(storage)
                .and_then(|v| Tag::new(v).map_err(domain))
        })
        .collect()
}

fn rebuild_fts(connection: &Connection) -> Result<()> {
    connection.execute_batch("DROP TABLE IF EXISTS node_fts; CREATE VIRTUAL TABLE node_fts USING fts5(node_id UNINDEXED, symbol, content, description, tags);").map_err(storage)?;
    connection.execute("INSERT INTO node_fts(node_id,symbol,content,description,tags) SELECT n.id,n.symbol,COALESCE(f.text,''),COALESCE(n.description,''),COALESCE((SELECT group_concat(tag,' ') FROM (SELECT tag FROM node_tags WHERE node_id=n.id ORDER BY tag)),'') FROM nodes n LEFT JOIN fragments f ON f.node_id=n.id ORDER BY n.id",[]).map_err(storage)?;
    Ok(())
}

fn find_node(connection: &Connection, symbol: &Symbol) -> Result<Option<(i64, &'static str, u64)>> {
    let value: Option<(i64, String, i64)> = connection
        .query_row(
            "SELECT id,kind,revision FROM nodes WHERE symbol=?1",
            [symbol.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(storage)?;
    value
        .map(|(id, k, r)| {
            let revision = u64::try_from(r).map_err(|_| {
                diag(
                    "E_DB_REVISION",
                    DiagnosticCategory::Internal,
                    "persisted revision is not positive",
                )
            })?;
            match k.as_str() {
                "fragment" => Ok((id, "fragment", revision)),
                "prompt" => Ok((id, "prompt", revision)),
                _ => Err(diag(
                    "E_DB_KIND",
                    DiagnosticCategory::Internal,
                    "unknown persisted node kind",
                )),
            }
        })
        .transpose()
}

fn bump_catalog(connection: &Connection) -> Result<()> {
    connection
        .execute(
            "UPDATE app_state SET value=value+1 WHERE key='catalog_revision'",
            [],
        )
        .map_err(storage)?;
    Ok(())
}
fn table_exists(connection: &Connection, name: &str) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
            [name],
            |r| r.get(0),
        )
        .map_err(storage)
}
fn pragma_i32(connection: &Connection, name: &str) -> Result<i32> {
    connection
        .pragma_query_value(None, name, |r| r.get(0))
        .map_err(storage)
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn is_constraint(error: &rusqlite::Error) -> bool {
    matches!(error,rusqlite::Error::SqliteFailure(e,_) if e.code==rusqlite::ErrorCode::ConstraintViolation)
}
fn storage(error: rusqlite::Error) -> Diagnostic {
    if matches!(&error, rusqlite::Error::SqliteFailure(code, _) if matches!(code.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))
    {
        return diag(
            "E_DB_BUSY",
            DiagnosticCategory::Storage,
            "database remained busy until the bounded wait expired; retry the operation",
        )
        .with_cause(error.to_string());
    }
    diag(
        "E_DB",
        DiagnosticCategory::Storage,
        "SQLite operation failed",
    )
    .with_cause(error.to_string())
}
fn domain(error: crate::domain::DomainError) -> Diagnostic {
    diag("E_DOMAIN", DiagnosticCategory::Domain, error.to_string())
}
fn diag(code: &str, category: DiagnosticCategory, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, category, message)
}
fn poisoned() -> Diagnostic {
    diag(
        "E_DB_LOCK",
        DiagnosticCategory::Internal,
        "SQLite connection mutex was poisoned",
    )
}
fn missing(symbol: &Symbol) -> Diagnostic {
    diag(
        "E_NODE_MISSING",
        DiagnosticCategory::Domain,
        format!("symbol `{symbol}` does not exist"),
    )
}
fn conflict(symbol: &Symbol, expected: Option<u64>, actual: Option<u64>) -> Diagnostic {
    diag(
        "E_REVISION_CONFLICT",
        DiagnosticCategory::Conflict,
        format!("stale revision for `{symbol}`: expected {expected:?}, actual {actual:?}"),
    )
}
fn revision_overflow() -> Diagnostic {
    diag(
        "E_REVISION_OVERFLOW",
        DiagnosticCategory::Internal,
        "node revision overflow",
    )
}
fn schema_new(found: i32) -> Diagnostic {
    diag(
        "E_SCHEMA_NEW",
        DiagnosticCategory::Compatibility,
        format!("database schema {found} is newer than supported schema {SCHEMA_VERSION}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(value: &str) -> Symbol {
        Symbol::new(value).unwrap()
    }
    fn text(value: &str) -> XmlText {
        XmlText::new(value).unwrap()
    }

    #[test]
    fn crud_preserves_duplicate_edges_rename_metadata_and_delete_rules() {
        let mut db = SqliteDatabase::open_in_memory().unwrap();
        db.write_transaction(&mut |writer| {
            writer.upsert_fragment(&symbol("Leaf"), &text("hello"), None)?;
            writer.replace_prompt(&symbol("Root"), &[symbol("Leaf"), symbol("Leaf")])?;
            Ok(vec![])
        })
        .unwrap();
        let snapshot = db.snapshot().unwrap();
        let root = snapshot.get_by_symbol(&symbol("Root")).unwrap();
        assert_eq!(
            snapshot.render_xml_vec(root.header.id).unwrap(),
            b"<Root><Leaf>hello</Leaf><Leaf>hello</Leaf></Root>\n"
        );

        db.write_transaction(&mut |writer| {
            writer.rename(&symbol("Leaf"), &symbol("Renamed"))?;
            writer.set_metadata(
                &symbol("Renamed"),
                &Metadata::new(
                    Some("desc".into()),
                    [Tag::new("z").unwrap(), Tag::new("a").unwrap()],
                ),
            )?;
            assert_eq!(
                writer.delete(&symbol("Renamed")).unwrap_err().code,
                "E_NODE_REFERENCED"
            );
            Ok(vec![])
        })
        .unwrap();
        let snapshot = db.snapshot().unwrap();
        let renamed = snapshot.get_by_symbol(&symbol("Renamed")).unwrap();
        assert_eq!(renamed.header.revision.get(), 3);
        assert_eq!(renamed.header.metadata.description(), Some("desc"));
        assert_eq!(
            renamed
                .header
                .metadata
                .tags()
                .map(Tag::as_str)
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
    }

    #[test]
    fn failed_closure_rolls_back_without_output_leakage() {
        let mut db = SqliteDatabase::open_in_memory().unwrap();
        let result = db.write_transaction(&mut |writer| {
            writer.upsert_fragment(&symbol("Transient"), &text("x"), None)?;
            Err(diag(
                "E_INJECTED",
                DiagnosticCategory::Internal,
                "injected failure",
            ))
        });
        assert_eq!(result.unwrap_err().code, "E_INJECTED");
        assert!(db.snapshot().unwrap().is_empty());
    }

    #[test]
    fn stale_revision_across_connections_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.db");
        let mut first = SqliteDatabase::open(&path).unwrap();
        first
            .write_transaction(&mut |writer| {
                writer.upsert_fragment(&symbol("Leaf"), &text("one"), None)?;
                Ok(vec![])
            })
            .unwrap();
        let mut second = SqliteDatabase::open(&path).unwrap();
        first
            .write_transaction(&mut |writer| {
                writer.upsert_fragment(
                    &symbol("Leaf"),
                    &text("two"),
                    Some(Revision::new(1).unwrap()),
                )?;
                Ok(vec![])
            })
            .unwrap();
        let error = second
            .write_transaction(&mut |writer| {
                writer.upsert_fragment(
                    &symbol("Leaf"),
                    &text("stale"),
                    Some(Revision::new(1).unwrap()),
                )?;
                Ok(vec![])
            })
            .unwrap_err();
        assert_eq!(error.code, "E_REVISION_CONFLICT");
        let snapshot = second.snapshot().unwrap();
        assert!(
            matches!(&snapshot.get_by_symbol(&symbol("Leaf")).unwrap().body, NodeBody::Fragment(value) if value.as_str() == "two")
        );
    }

    #[test]
    fn competing_writer_gets_stable_busy_diagnostic() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.db");
        let first = SqliteDatabase::open(&path).unwrap();
        let mut second = SqliteDatabase::open(&path).unwrap();
        second
            .lock()
            .unwrap()
            .busy_timeout(Duration::from_millis(5))
            .unwrap();
        let first_connection = first.lock().unwrap();
        first_connection.execute_batch("BEGIN IMMEDIATE").unwrap();
        let error = second.write_transaction(&mut |_| Ok(vec![])).unwrap_err();
        assert_eq!(error.code, "E_DB_BUSY");
        first_connection.execute_batch("ROLLBACK").unwrap();
    }

    #[test]
    fn migration_pragmas_backup_and_reversed_selects_are_valid() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.db");
        let backup = directory.path().join("backup.db");
        let mut db = SqliteDatabase::open(&path).unwrap();
        let status = db.status().unwrap();
        assert_eq!(
            (
                status.application_id,
                status.user_version,
                status.ledger_version
            ),
            (APPLICATION_ID, SCHEMA_VERSION, Some(SCHEMA_VERSION))
        );
        {
            let conn = db.lock().unwrap();
            assert_eq!(pragma_i32(&conn, "foreign_keys").unwrap(), 1);
            assert_eq!(pragma_i32(&conn, "trusted_schema").unwrap(), 0);
            conn.pragma_update(None, "reverse_unordered_selects", true)
                .unwrap();
        }
        db.write_transaction(&mut |writer| {
            writer.upsert_fragment(&symbol("B"), &text("b"), None)?;
            writer.upsert_fragment(&symbol("A"), &text("a"), None)?;
            writer.replace_prompt(&symbol("Root"), &[symbol("B"), symbol("A"), symbol("B")])?;
            Ok(vec![])
        })
        .unwrap();
        assert_eq!(
            db.snapshot()
                .unwrap()
                .iter_by_symbol()
                .map(|n| n.header.symbol.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "Root"]
        );
        db.backup(&backup).unwrap();
        let restored = SqliteDatabase::open(&backup).unwrap();
        assert_eq!(restored.snapshot().unwrap().len(), 3);
        let report = restored.check(false).unwrap();
        assert_eq!(report.integrity, ["ok"]);
        assert_eq!(report.foreign_key_violations, 0);
        assert!(report.domain_valid);
    }

    #[test]
    fn version_mirror_can_be_repaired_but_bad_checksum_cannot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.db");
        let db = SqliteDatabase::open(&path).unwrap();
        db.lock()
            .unwrap()
            .pragma_update(None, "user_version", 0)
            .unwrap();
        drop(db);
        let mut db = SqliteDatabase::open(&path).unwrap();
        assert_eq!(db.snapshot().unwrap_err().code, "E_DB_VERSION_MIRROR");
        db.repair_derived().unwrap();
        assert!(db.snapshot().is_ok());

        db.lock()
            .unwrap()
            .execute(
                "UPDATE schema_migrations SET checksum='bad' WHERE version=1",
                [],
            )
            .unwrap();
        drop(db);
        let mut db = SqliteDatabase::open(&path).unwrap();
        assert_eq!(db.repair_derived().unwrap_err().code, "E_DB_MIGRATION");
    }
}
