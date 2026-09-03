//! Promptr 应用门面。 / Promptr application facade.

use std::{fs, path::PathBuf};

use crate::{
    Diagnostic, DiagnosticCategory,
    diagnostic::Result,
    infrastructure::{
        config::Config,
        sqlite::{SqliteDatabase, SqliteOptions},
    },
};

use super::{CheckedProgram, Effects, Value, ports::Database};
use crate::infrastructure::editor::TextProvider;

/// @brief 一次调用允许使用的能力。 / Capabilities allowed for one invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationPolicy {
    /// @brief 允许的效果集合。 / Allowed effect set.
    pub allowed_effects: Effects,
}

impl InvocationPolicy {
    /// @brief 构造非交互脚本策略。 / Construct a non-interactive script policy.
    /// @return 允许目录读写但不允许交互的策略。 / Policy permitting catalog I/O but not interaction.
    #[must_use]
    pub const fn script() -> Self {
        Self {
            allowed_effects: Effects::READ_STORE.union(Effects::WRITE_STORE),
        }
    }

    /// @brief 构造完全交互策略。 / Construct a fully interactive policy.
    /// @return 允许所有核心效果的策略。 / Policy permitting every core effect.
    #[must_use]
    pub const fn interactive() -> Self {
        Self {
            allowed_effects: Effects::all(),
        }
    }
}

/// @brief 打开应用所需的显式选项。 / Explicit options used to open the application.
#[derive(Clone, Debug)]
pub struct PromptrOptions {
    /// @brief 数据库路径覆盖。 / Database path override.
    pub database_path: Option<PathBuf>,
    /// @brief 已完成覆盖与验证的配置。 / Fully overlaid and validated configuration.
    pub config: Config,
}

/// @brief 同步、单语义路径的应用门面。 / Synchronous application facade with one semantic path.
pub struct Promptr {
    /// @brief 持久化端口。 / Persistence port.
    pub(crate) database: Box<dyn Database>,
    /// @brief 生效配置。 / Effective configuration.
    pub(crate) config: Config,
}

impl Promptr {
    /// @brief 使用生产 SQLite 适配器打开应用。 / Open the application with the production SQLite adapter.
    /// @param options 数据库覆盖与已验证配置。 / Database override and validated configuration.
    /// @return 已装配应用或结构化诊断。 / Assembled application or a structured diagnostic.
    /// @example 从配置打开可复用库门面。 / Open the reusable library facade from configuration.
    /// ```no_run
    /// # use promptr::{Diagnostic, Promptr, PromptrOptions};
    /// # use promptr::infrastructure::config::Config;
    /// # fn example(config: Config) -> Result<(), Diagnostic> {
    /// let mut app = Promptr::open(PromptrOptions {
    ///     database_path: Some("catalog.sqlite".into()),
    ///     config,
    /// })?;
    /// let values = app.eval("LIST;", promptr::InvocationPolicy::script())?;
    /// # let _ = values;
    /// # Ok(())
    /// # }
    /// ```
    pub fn open(options: PromptrOptions) -> Result<Self> {
        let PromptrOptions {
            database_path,
            mut config,
        } = options;
        let database_path = database_path.unwrap_or_else(|| config.database.path.clone());
        if let Some(parent) = database_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| {
                Diagnostic::error(
                    "E_DATABASE_PARENT",
                    DiagnosticCategory::External,
                    format!("could not create database directory `{}`", parent.display()),
                )
                .with_cause(error.to_string())
            })?;
        }
        config.database.path = database_path.clone();
        let sqlite_options = SqliteOptions::from(&config.database);
        let database = SqliteDatabase::open_with_options(database_path, sqlite_options)?;
        Ok(Self::from_parts(Box::new(database), config))
    }

    /// @brief 从已装配依赖构造门面。 / Construct the facade from assembled dependencies.
    /// @param database 数据库端口。 / Database port.
    /// @param config 已验证配置。 / Validated configuration.
    /// @return 应用门面。 / Application facade.
    #[must_use]
    pub fn from_parts(database: Box<dyn Database>, config: Config) -> Self {
        Self { database, config }
    }

    /// @brief 执行源程序；完整实现由运行时协调器提供。 / Evaluate source; the runtime coordinator provides the complete implementation.
    /// @param source DSL 源码。 / DSL source.
    /// @param policy 调用能力策略。 / Invocation capability policy.
    /// @return 类型化值或结构化诊断。 / Typed values or structured diagnostic.
    pub fn eval(&mut self, source: &str, policy: InvocationPolicy) -> Result<Vec<Value>> {
        crate::application::runtime::eval(self, source, policy, None)
    }

    /// @brief 使用显式文本提供者执行源码。 / Evaluate source with an explicit text provider.
    /// @param source DSL 源码。 / DSL source.
    /// @param policy 调用能力策略。 / Invocation capability policy.
    /// @param provider 不持有存储引用的文本提供者。 / Text provider that owns no store reference.
    /// @return 提交后可发布的类型化值。 / Typed values publishable after commit.
    pub fn eval_with_provider(
        &mut self,
        source: &str,
        policy: InvocationPolicy,
        provider: &mut dyn TextProvider,
    ) -> Result<Vec<Value>> {
        crate::application::runtime::eval(self, source, policy, Some(provider))
    }

    /// @brief 解析并按当前目录快照检查源码但不执行。 / Parse and check source against the current catalog snapshot without executing it.
    /// @param source DSL 源码。 / DSL source.
    /// @param policy 调用能力策略。 / Invocation capability policy.
    /// @return 已检查程序或稳定诊断。 / Checked program or stable diagnostic.
    pub fn check(&self, source: &str, policy: InvocationPolicy) -> Result<CheckedProgram> {
        crate::application::runtime::check(self, source, policy)
    }

    /// @brief 借用生效配置。 / Borrow the effective configuration.
    /// @return 生效配置。 / Effective configuration.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// @brief 查询持久化适配器的外部变化令牌。 / Query the persistence adapter's external-change token.
    /// @return 支持时返回令牌，否则返回 None。 / Token when supported, otherwise None.
    pub fn change_token(&self) -> Result<Option<u64>> {
        self.database.change_token()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::config::ConfigPaths;

    /// @brief 构造隔离的默认配置。 / Build isolated default configuration.
    fn config(root: &std::path::Path) -> Config {
        Config::defaults(&ConfigPaths {
            user_config: root.join("config.toml"),
            database: root.join("configured.sqlite"),
            state_dir: root.join("state"),
            cache_dir: root.join("cache"),
            backup_dir: root.join("backup"),
        })
    }

    #[test]
    fn open_honors_database_override_and_creates_parent() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("nested").join("override.sqlite");
        let mut app = Promptr::open(PromptrOptions {
            database_path: Some(database.clone()),
            config: config(directory.path()),
        })
        .unwrap();

        assert_eq!(app.config().database.path, database);
        assert!(app.config().database.path.exists());
        assert_eq!(
            app.eval("LIST;", InvocationPolicy::script()).unwrap().len(),
            1
        );
    }

    #[test]
    fn open_uses_configured_database_without_an_override() {
        let directory = tempfile::tempdir().unwrap();
        let config = config(directory.path());
        let configured = config.database.path.clone();
        let app = Promptr::open(PromptrOptions {
            database_path: None,
            config,
        })
        .unwrap();

        assert_eq!(app.config().database.path, configured);
        assert!(configured.exists());
    }
}
