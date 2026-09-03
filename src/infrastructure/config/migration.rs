//! 配置迁移 / Configuration migration.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::NamedTempFile;
use toml_edit::{DocumentMut, value};

use super::{CURRENT_SCHEMA_VERSION, ConfigDiagnostic, ParsedConfig};

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
    let version = parsed.raw.schema_version.unwrap_or(CURRENT_SCHEMA_VERSION);
    if version > CURRENT_SCHEMA_VERSION {
        parsed.raw.clone().validate()?;
    }
    if version < CURRENT_SCHEMA_VERSION {
        Ok(MigrationOutcome::WouldMigrate {
            from: version,
            to: CURRENT_SCHEMA_VERSION,
        })
    } else {
        parsed.raw.validate()?;
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
    let from = parsed.raw.schema_version.unwrap_or(CURRENT_SCHEMA_VERSION);
    if from > CURRENT_SCHEMA_VERSION {
        parsed.raw.clone().validate()?;
    }
    if from == CURRENT_SCHEMA_VERSION {
        parsed.raw.validate()?;
        return Ok(MigrationOutcome::AlreadyCurrent);
    }
    let mut document = parsed.document;
    upgrade_document(&mut document, from)?;
    let migrated = document.to_string();
    let checked = ParsedConfig::parse(&migrated, path.display().to_string())?;
    checked.raw.validate()?;

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

struct ConfigLock {
    path: PathBuf,
}

impl ConfigLock {
    fn acquire(path: &Path) -> Result<Self, Vec<ConfigDiagnostic>> {
        let lock_path = path.with_extension("toml.lock");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|error| {
                vec![
                    ConfigDiagnostic::error(
                        "E_CONFIG_LOCK",
                        format!(
                            "cannot acquire migration lock `{}`: {error}",
                            lock_path.display()
                        ),
                    )
                    .with_suggestion("wait for the other config migration to finish"),
                ]
            })?;
        Ok(Self { path: lock_path })
    }
}

impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
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
}
