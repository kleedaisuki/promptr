//! 外部系统适配器。 / Adapters for external systems.

pub mod config;
pub mod editor;
pub mod sqlite;

use std::time::Duration;

use config::{DatabaseConfig, JournalMode};
use sqlite::{SqliteJournalMode, SqliteOptions};

/// 将已验证数据库配置映射为 SQLite 适配器选项。 /
/// Maps validated database configuration to SQLite-adapter options.
///
/// <!-- @brief 将已验证数据库配置映射为 SQLite 适配器选项。 / Map validated database configuration to SQLite-adapter options. -->
impl From<&DatabaseConfig> for SqliteOptions {
    /// 执行无失败的强类型选项映射。 / Performs the infallible strongly typed option mapping.
    ///
    /// # Arguments
    ///
    /// * `config` - 已完成语义验证的数据库配置。 /
    ///   Database configuration that has passed semantic validation.
    ///
    /// # Returns
    ///
    /// 与配置表示解耦的 SQLite 打开选项。 /
    /// SQLite open options decoupled from the configuration representation.
    ///
    /// <!-- @brief 执行无失败的强类型选项映射。 / Perform the infallible strongly typed option mapping. -->
    /// <!-- @param config 已完成语义验证的数据库配置。 / Database configuration that has passed semantic validation. -->
    /// <!-- @return 与配置表示解耦的 SQLite 打开选项。 / SQLite open options decoupled from the configuration representation. -->
    fn from(config: &DatabaseConfig) -> Self {
        Self {
            journal_mode: match config.journal_mode {
                JournalMode::Wal => SqliteJournalMode::Wal,
                JournalMode::Delete => SqliteJournalMode::Delete,
            },
            busy_timeout: Duration::from_millis(config.busy_timeout_ms),
            auto_migrate: config.auto_migrate,
            backup_before_migrate: config.backup_before_migrate,
            backup_keep: config.backup_keep,
        }
    }
}
