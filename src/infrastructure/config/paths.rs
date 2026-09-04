//! 平台路径 / Platform paths.

use std::path::PathBuf;

use directories::ProjectDirs;

use super::ConfigDiagnostic;

/// Promptr 的平台目录集 / Promptr platform directory set.
///
/// <!-- @brief Promptr 的平台目录集 / Promptr platform directory set. -->
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    /// 用户配置文件 / User configuration file.
    ///
    /// <!-- @brief 用户配置文件 / User configuration file. -->
    pub user_config: PathBuf,
    /// 默认数据库 / Default database file.
    ///
    /// <!-- @brief 默认数据库 / Default database file. -->
    pub database: PathBuf,
    /// 状态目录 / State directory.
    ///
    /// <!-- @brief 状态目录 / State directory. -->
    pub state_dir: PathBuf,
    /// 缓存目录 / Cache directory.
    ///
    /// <!-- @brief 缓存目录 / Cache directory. -->
    pub cache_dir: PathBuf,
    /// 备份目录 / Backup directory.
    ///
    /// <!-- @brief 备份目录 / Backup directory. -->
    pub backup_dir: PathBuf,
}

impl ConfigPaths {
    /// 使用当前平台约定解析目录 / Resolves directories using platform conventions.
    ///
    /// <!-- @brief 使用当前平台约定解析目录 / Resolves directories using platform conventions. -->
    /// # Returns
    ///
    /// 互不混用的配置、数据、状态、缓存和备份路径 / Distinct config, data, state,
    /// cache, and backup paths.
    ///
    /// # Errors
    ///
    /// 当前平台无法确定应用目录时返回配置诊断 / Returns a configuration diagnostic
    /// when the current platform cannot determine application directories.
    ///
    /// <!-- @return 互不混用的配置、数据、状态、缓存和备份路径 / Distinct config, data, state, cache, and backup paths. -->
    pub fn discover() -> Result<Self, ConfigDiagnostic> {
        let project = ProjectDirs::from("org", "promptr", "promptr").ok_or_else(|| {
            ConfigDiagnostic::error(
                "E_CONFIG_DIRS",
                "cannot determine platform application directories",
            )
        })?;
        let state_dir = project
            .state_dir()
            .map(PathBuf::from)
            .unwrap_or_else(|| project.data_local_dir().join("state"));
        Ok(Self {
            user_config: project.config_dir().join("config.toml"),
            database: project.data_dir().join("promptr.db"),
            backup_dir: state_dir.join("backups"),
            state_dir,
            cache_dir: project.cache_dir().to_path_buf(),
        })
    }
}
