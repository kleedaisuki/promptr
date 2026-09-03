//! Promptr 应用门面。 / Promptr application facade.

use std::{fs, path::PathBuf};

use crate::{
    Diagnostic, DiagnosticCategory,
    diagnostic::Result,
    domain::{NodeId, Revision, Symbol},
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

/// @brief 节点操作的乐观修订前置条件。 / Optimistic revision precondition for a node operation.
/// @note 符号绑定、稳定标识和修订号必须同时匹配；重命名、删除或替换身份都会产生冲突。 / The symbol binding, stable identity, and revision must all match; rename, deletion, or identity replacement is a conflict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodePrecondition {
    /// @brief 调用方观察到的符号绑定。 / Symbol binding observed by the caller.
    symbol: Symbol,
    /// @brief 调用方观察到的稳定节点标识。 / Stable node identity observed by the caller.
    node_id: NodeId,
    /// @brief 调用方观察到的节点修订号。 / Node revision observed by the caller.
    revision: Revision,
}

impl NodePrecondition {
    /// @brief 构造节点修订前置条件。 / Construct a node revision precondition.
    /// @param symbol 调用方观察到的符号。 / Symbol observed by the caller.
    /// @param node_id 调用方观察到的稳定标识。 / Stable identity observed by the caller.
    /// @param revision 调用方观察到的修订号。 / Revision observed by the caller.
    /// @return 强类型前置条件。 / Strongly typed precondition.
    #[must_use]
    pub const fn new(symbol: Symbol, node_id: NodeId, revision: Revision) -> Self {
        Self {
            symbol,
            node_id,
            revision,
        }
    }

    /// @brief 借用调用方观察到的符号。 / Borrow the symbol observed by the caller.
    /// @return 符号。 / Symbol.
    #[must_use]
    pub const fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// @brief 返回调用方观察到的节点标识。 / Return the node identity observed by the caller.
    /// @return 节点标识。 / Node identity.
    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// @brief 返回调用方观察到的修订号。 / Return the revision observed by the caller.
    /// @return 修订号。 / Revision.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }
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
        database.validate_operational()?;
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
        self.eval_preconditioned(source, policy, &[])
    }

    /// @brief 在节点修订前置条件下执行源程序。 / Evaluate source under node revision preconditions.
    /// @param source DSL 源码。 / DSL source.
    /// @param policy 调用能力策略。 / Invocation capability policy.
    /// @param preconditions 调用方观察到的节点身份与修订。 / Node identities and revisions observed by the caller.
    /// @return 条件仍成立时返回类型化值，否则返回 `E_CONFLICT` 且整批不执行。 / Typed values when conditions still hold; otherwise `E_CONFLICT` with no batch execution.
    pub fn eval_preconditioned(
        &mut self,
        source: &str,
        policy: InvocationPolicy,
        preconditions: &[NodePrecondition],
    ) -> Result<Vec<Value>> {
        crate::application::runtime::eval(self, source, policy, None, preconditions)
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
        self.eval_with_provider_preconditioned(source, policy, provider, &[])
    }

    /// @brief 使用文本提供者并在节点修订前置条件下执行源码。 / Evaluate source with a text provider under node revision preconditions.
    /// @param source DSL 源码。 / DSL source.
    /// @param policy 调用能力策略。 / Invocation capability policy.
    /// @param provider 不持有存储引用的文本提供者。 / Text provider that owns no store reference.
    /// @param preconditions 调用方观察到的节点身份与修订。 / Node identities and revisions observed by the caller.
    /// @return 条件仍成立时返回提交后可发布的值，否则返回 `E_CONFLICT` 且整批不执行。 / Values publishable after commit when conditions still hold; otherwise `E_CONFLICT` with no batch execution.
    pub fn eval_with_provider_preconditioned(
        &mut self,
        source: &str,
        policy: InvocationPolicy,
        provider: &mut dyn TextProvider,
        preconditions: &[NodePrecondition],
    ) -> Result<Vec<Value>> {
        crate::application::runtime::eval(self, source, policy, Some(provider), preconditions)
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

    #[test]
    fn operational_open_rejects_a_newer_schema_before_exposing_the_facade() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("future.sqlite");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(&format!(
                "PRAGMA application_id={}; PRAGMA user_version={};",
                crate::infrastructure::sqlite::APPLICATION_ID,
                crate::infrastructure::sqlite::SCHEMA_VERSION + 1
            ))
            .unwrap();
        drop(connection);

        let error = match Promptr::open(PromptrOptions {
            database_path: Some(path.clone()),
            config: config(directory.path()),
        }) {
            Ok(_) => panic!("newer schema unexpectedly produced an operational facade"),
            Err(error) => error,
        };

        assert_eq!(error.code, "E_SCHEMA_NEW");
        assert!(error.message.contains(&path.display().to_string()));
        assert!(error.message.contains("found=2"));
        assert!(error.message.contains("max=1"));
        assert!(error.message.contains(env!("CARGO_PKG_VERSION")));
    }
}
