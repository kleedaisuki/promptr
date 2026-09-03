//! 配置迁移 / Configuration migration.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use tempfile::NamedTempFile;
use toml_edit::{DocumentMut, value};

use super::{
    CURRENT_SCHEMA_VERSION, ConfigDiagnostic, ConfigLayer, ConfigLoader, ConfigPaths, ConfigSource,
    ParsedConfig, RawConfig,
};

/// @brief 配置迁移操作结果 / Configuration migration operation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// @brief 文件已是当前版本 / File is already current.
    AlreadyCurrent,
    /// @brief 检查模式发现需要迁移 / Check mode found a pending migration.
    WouldMigrate {
        /// @brief 原版本 / Original version.
        from: u32,
        /// @brief 目标版本 / Target version.
        to: u32,
    },
    /// @brief 已原子替换并保留备份 / File was atomically replaced and a backup retained.
    Migrated {
        /// @brief 原版本 / Original version.
        from: u32,
        /// @brief 目标版本 / Target version.
        to: u32,
        /// @brief 原始字节备份 / Backup containing the original bytes.
        backup: PathBuf,
    },
}

/// @brief 检查配置可解析性和迁移需求，不写文件 / Checks parseability and migration need without writing.
/// @param path 配置路径 / Configuration path.
/// @return 检查结果或诊断 / Check outcome or diagnostics.
pub fn check_file(path: &Path) -> Result<MigrationOutcome, Vec<ConfigDiagnostic>> {
    let text = fs::read_to_string(path).map_err(|error| io_diagnostics(path, error))?;
    let parsed = ParsedConfig::parse(&text, path.display().to_string())?;
    if !parsed.diagnostics.is_empty() {
        return Err(parsed.diagnostics);
    }
    validate_standalone(parsed.raw, path)?;
    let version = parsed.input_schema_version;
    if version < CURRENT_SCHEMA_VERSION {
        Ok(MigrationOutcome::WouldMigrate {
            from: version,
            to: CURRENT_SCHEMA_VERSION,
        })
    } else {
        Ok(MigrationOutcome::AlreadyCurrent)
    }
}

/// @brief 显式迁移配置，保留原文备份 / Explicitly migrates configuration while retaining original bytes.
/// @param path 配置路径 / Configuration path.
/// @return 迁移结果或诊断 / Migration outcome or diagnostics.
/// @note 普通加载不调用此函数 / Ordinary loading does not call this function.
pub fn migrate_file(path: &Path) -> Result<MigrationOutcome, Vec<ConfigDiagnostic>> {
    let _lock = ConfigLock::acquire(path)?;
    let original = fs::read(path).map_err(|error| io_diagnostics(path, error))?;
    let text = std::str::from_utf8(&original).map_err(|error| {
        vec![
            ConfigDiagnostic::error("E_CONFIG_UTF8", error.to_string())
                .with_source(path.display().to_string()),
        ]
    })?;
    let parsed = ParsedConfig::parse(text, path.display().to_string())?;
    if !parsed.diagnostics.is_empty() {
        return Err(parsed.diagnostics);
    }
    let from = parsed.input_schema_version;
    if from == CURRENT_SCHEMA_VERSION {
        validate_standalone(parsed.raw, path)?;
        return Ok(MigrationOutcome::AlreadyCurrent);
    }
    let mut document = parsed.document;
    upgrade_document(&mut document, from)?;
    let migrated = document.to_string();
    let checked = ParsedConfig::parse(&migrated, path.display().to_string())?;
    validate_standalone(checked.raw, path)?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary =
        NamedTempFile::new_in(parent).map_err(|error| io_diagnostics(path, error))?;
    temporary
        .write_all(migrated.as_bytes())
        .map_err(|error| io_diagnostics(path, error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| io_diagnostics(path, error))?;
    set_restrictive_permissions(temporary.path()).map_err(|error| io_diagnostics(path, error))?;
    let backup = backup_path(path);
    fs::write(&backup, &original).map_err(|error| io_diagnostics(&backup, error))?;
    OpenOptions::new()
        .write(true)
        .open(&backup)
        .and_then(|file| file.sync_all())
        .map_err(|error| io_diagnostics(&backup, error))?;
    temporary
        .persist(path)
        .map_err(|error| io_diagnostics(path, error.error))?;
    sync_parent(parent);
    Ok(MigrationOutcome::Migrated {
        from,
        to: CURRENT_SCHEMA_VERSION,
        backup,
    })
}

pub(super) fn upgrade_document(
    document: &mut DocumentMut,
    from: u32,
) -> Result<(), Vec<ConfigDiagnostic>> {
    let mut version = from;
    while version < CURRENT_SCHEMA_VERSION {
        match version {
            0 => {
                document["schema_version"] = value(1);
                version = 1;
            }
            _ => {
                return Err(vec![ConfigDiagnostic::error(
                    "E_CONFIG_MIGRATION",
                    format!("no migration from schema {version}"),
                )]);
            }
        }
    }
    Ok(())
}

/// @brief 按完整独立配置语义验证迁移结果 / Validates a migration result as a complete standalone configuration.
/// @param raw 已升级至当前模式的原始配置 / Raw configuration upgraded to the current schema.
/// @param path 配置文件路径 / Configuration file path.
/// @return 配置有效时返回空值，否则返回全部诊断 / Unit on success, or all diagnostics.
/// @note 普通分层加载仍允许跨层补全字段；该检查仅用于独立文件检查与迁移 / Ordinary layered loading may still complete fields across layers; this check is only for standalone checking and migration.
fn validate_standalone(raw: RawConfig, path: &Path) -> Result<(), Vec<ConfigDiagnostic>> {
    let source = path.display().to_string();
    let overlay = raw.validate_at(&source)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let paths = ConfigPaths {
        user_config: path.to_path_buf(),
        database: parent.join("promptr.db"),
        state_dir: parent.to_path_buf(),
        cache_dir: parent.to_path_buf(),
        backup_dir: parent.to_path_buf(),
    };
    ConfigLoader::new(paths)
        .merge(vec![ConfigLayer {
            source: ConfigSource::ExplicitFile(path.to_path_buf()),
            overlay,
        }])
        .map(|_| ())
        .map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| {
                    if diagnostic.source.is_none() {
                        diagnostic.with_source(source.clone())
                    } else {
                        diagnostic
                    }
                })
                .collect()
        })
}

struct ConfigLock {
    file: fs::File,
}

impl ConfigLock {
    fn acquire(path: &Path) -> Result<Self, Vec<ConfigDiagnostic>> {
        let lock_path = path.with_extension("toml.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| io_diagnostics(&lock_path, error))?;
        file.try_lock_exclusive().map_err(|error| {
            vec![
                ConfigDiagnostic::error(
                    "E_CONFIG_LOCK",
                    format!(
                        "cannot acquire migration lock `{}`: {error}",
                        lock_path.display()
                    ),
                )
                .with_suggestion("another migration is active; retry after it completes"),
            ]
        })?;
        Ok(Self { file })
    }
}

impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn backup_path(path: &Path) -> PathBuf {
    let stamp = now_nanos();
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}.bak.{stamp}"))
}

fn io_diagnostics(path: &Path, error: std::io::Error) -> Vec<ConfigDiagnostic> {
    vec![
        ConfigDiagnostic::error("E_CONFIG_IO", format!("`{}`: {error}", path.display()))
            .with_source(path.display().to_string()),
    ]
}

#[cfg(unix)]
fn set_restrictive_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_restrictive_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn sync_parent(parent: &Path) {
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_never_rewrites_old_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "schema_version = 0\n# retained\n").unwrap();
        let before = fs::read(&path).unwrap();
        assert!(matches!(
            check_file(&path).unwrap(),
            MigrationOutcome::WouldMigrate { from: 0, to: 1 }
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn check_validates_migrated_old_config_without_rewriting() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let original = b"schema_version = 0\n[editor]\nmode = \"external\"\n";
        fs::write(&path, original).unwrap();

        let diagnostics = check_file(&path).unwrap_err();

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E_CONFIG_EDITOR")
        );
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn explicit_migration_validates_before_writing_or_backing_up() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let original = b"schema_version = 0\n[editor]\nmode = \"external\"\n";
        fs::write(&path, original).unwrap();

        let diagnostics = migrate_file(&path).unwrap_err();

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E_CONFIG_EDITOR")
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!fs::read_dir(directory.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".bak.")
        }));
    }

    #[test]
    fn explicit_migration_retains_backup_and_comments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let original = b"schema_version = 0\n# retained\n";
        fs::write(&path, original).unwrap();
        let outcome = migrate_file(&path).unwrap();
        let MigrationOutcome::Migrated { backup, .. } = outcome else {
            panic!("expected migration")
        };
        assert_eq!(fs::read(backup).unwrap(), original);
        let result = fs::read_to_string(path).unwrap();
        assert!(result.contains("# retained"));
        assert!(result.contains("schema_version = 1"));
    }

    #[test]
    fn migration_lock_competes_and_drop_releases_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let lock_path = path.with_extension("toml.lock");
        let first = ConfigLock::acquire(&path).unwrap();
        assert!(ConfigLock::acquire(&path).is_err());
        drop(first);
        let second = ConfigLock::acquire(&path).unwrap();
        drop(second);
        assert!(lock_path.exists());
    }
}
