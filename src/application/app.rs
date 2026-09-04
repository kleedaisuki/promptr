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

/// 一次调用允许使用的能力。 / Capabilities allowed for one invocation.
///
/// <!-- @brief 一次调用允许使用的能力。 / Capabilities allowed for one invocation. -->
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationPolicy {
    /// 允许的效果集合。 / Allowed effect set.
    ///
    /// <!-- @brief 允许的效果集合。 / Allowed effect set. -->
    pub allowed_effects: Effects,
}

impl InvocationPolicy {
    /// 构造非交互脚本策略。 / Construct a non-interactive script policy.
    ///
    /// <!-- @brief 构造非交互脚本策略。 / Construct a non-interactive script policy. -->
    ///
    /// # Returns
    /// 允许目录读写但不允许交互的策略。 / Policy permitting catalog I/O but not interaction.
    ///
    /// <!-- @return 允许目录读写但不允许交互的策略。 / Policy permitting catalog I/O but not interaction. -->
    #[must_use]
    pub const fn script() -> Self {
        Self {
            allowed_effects: Effects::READ_STORE.union(Effects::WRITE_STORE),
        }
    }

    /// 构造完全交互策略。 / Construct a fully interactive policy.
    ///
    /// <!-- @brief 构造完全交互策略。 / Construct a fully interactive policy. -->
    ///
    /// # Returns
    /// 允许所有核心效果的策略。 / Policy permitting every core effect.
    ///
    /// <!-- @return 允许所有核心效果的策略。 / Policy permitting every core effect. -->
    #[must_use]
    pub const fn interactive() -> Self {
        Self {
            allowed_effects: Effects::all(),
        }
    }
}

/// 打开应用所需的显式选项。 / Explicit options used to open the application.
///
/// <!-- @brief 打开应用所需的显式选项。 / Explicit options used to open the application. -->
#[derive(Clone, Debug)]
pub struct PromptrOptions {
    /// 数据库路径覆盖。 / Database path override.
    ///
    /// <!-- @brief 数据库路径覆盖。 / Database path override. -->
    pub database_path: Option<PathBuf>,
    /// 已完成覆盖与验证的配置。 / Fully overlaid and validated configuration.
    ///
    /// <!-- @brief 已完成覆盖与验证的配置。 / Fully overlaid and validated configuration. -->
    pub config: Config,
}

/// 同步、单语义路径的应用门面。 / Synchronous application facade with one semantic path.
///
/// <!-- @brief 同步、单语义路径的应用门面。 / Synchronous application facade with one semantic path. -->
pub struct Promptr {
    /// 持久化端口。 / Persistence port.
    ///
    /// <!-- @brief 持久化端口。 / Persistence port. -->
    pub(crate) database: Box<dyn Database>,
    /// 生效配置。 / Effective configuration.
    ///
    /// <!-- @brief 生效配置。 / Effective configuration. -->
    pub(crate) config: Config,
}

/// 节点操作的乐观修订前置条件。 / Optimistic revision precondition for a node operation.
///
/// <!-- @brief 节点操作的乐观修订前置条件。 / Optimistic revision precondition for a node operation. -->
///
/// # Notes
/// 符号绑定、稳定标识和修订号必须同时匹配；重命名、删除或替换身份都会产生冲突。 / The symbol binding, stable identity, and revision must all match; rename, deletion, or identity replacement is a conflict.
///
/// <!-- @note 符号绑定、稳定标识和修订号必须同时匹配；重命名、删除或替换身份都会产生冲突。 / The symbol binding, stable identity, and revision must all match; rename, deletion, or identity replacement is a conflict. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodePrecondition {
    /// 调用方观察到的符号绑定。 / Symbol binding observed by the caller.
    ///
    /// <!-- @brief 调用方观察到的符号绑定。 / Symbol binding observed by the caller. -->
    symbol: Symbol,
    /// 调用方观察到的稳定节点标识。 / Stable node identity observed by the caller.
    ///
    /// <!-- @brief 调用方观察到的稳定节点标识。 / Stable node identity observed by the caller. -->
    node_id: NodeId,
    /// 调用方观察到的节点修订号。 / Node revision observed by the caller.
    ///
    /// <!-- @brief 调用方观察到的节点修订号。 / Node revision observed by the caller. -->
    revision: Revision,
}

impl NodePrecondition {
    /// 构造节点修订前置条件。 / Construct a node revision precondition.
    ///
    /// <!-- @brief 构造节点修订前置条件。 / Construct a node revision precondition. -->
    ///
    /// # Arguments
    /// - `symbol`: 调用方观察到的符号。 / Symbol observed by the caller.
    /// <!-- @param symbol 调用方观察到的符号。 / Symbol observed by the caller. -->
    /// - `node_id`: 调用方观察到的稳定标识。 / Stable identity observed by the caller.
    /// <!-- @param node_id 调用方观察到的稳定标识。 / Stable identity observed by the caller. -->
    /// - `revision`: 调用方观察到的修订号。 / Revision observed by the caller.
    /// <!-- @param revision 调用方观察到的修订号。 / Revision observed by the caller. -->
    ///
    /// # Returns
    /// 强类型前置条件。 / Strongly typed precondition.
    ///
    /// <!-- @return 强类型前置条件。 / Strongly typed precondition. -->
    #[must_use]
    pub const fn new(symbol: Symbol, node_id: NodeId, revision: Revision) -> Self {
        Self {
            symbol,
            node_id,
            revision,
        }
    }

    /// 借用调用方观察到的符号。 / Borrow the symbol observed by the caller.
    ///
    /// <!-- @brief 借用调用方观察到的符号。 / Borrow the symbol observed by the caller. -->
    ///
    /// # Returns
    /// 符号。 / Symbol.
    ///
    /// <!-- @return 符号。 / Symbol. -->
    #[must_use]
    pub const fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// 返回调用方观察到的节点标识。 / Return the node identity observed by the caller.
    ///
    /// <!-- @brief 返回调用方观察到的节点标识。 / Return the node identity observed by the caller. -->
    ///
    /// # Returns
    /// 节点标识。 / Node identity.
    ///
    /// <!-- @return 节点标识。 / Node identity. -->
    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// 返回调用方观察到的修订号。 / Return the revision observed by the caller.
    ///
    /// <!-- @brief 返回调用方观察到的修订号。 / Return the revision observed by the caller. -->
    ///
    /// # Returns
    /// 修订号。 / Revision.
    ///
    /// <!-- @return 修订号。 / Revision. -->
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }
}

impl Promptr {
    /// 使用生产 SQLite 适配器打开应用。 / Open the application with the production SQLite adapter.
    ///
    /// <!-- @brief 使用生产 SQLite 适配器打开应用。 / Open the application with the production SQLite adapter. -->
    ///
    /// # Arguments
    /// - `options`: 数据库覆盖与已验证配置。 / Database override and validated configuration.
    /// <!-- @param options 数据库覆盖与已验证配置。 / Database override and validated configuration. -->
    ///
    /// # Returns
    /// 已装配应用或结构化诊断。 / Assembled application or a structured diagnostic.
    ///
    /// <!-- @return 已装配应用或结构化诊断。 / Assembled application or a structured diagnostic. -->
    ///
    /// # Errors
    /// 当数据库父目录无法创建、SQLite 数据库无法打开，或数据库模式不可用于正常操作时，返回结构化诊断。 /
    /// Returns a structured diagnostic when the database parent cannot be created, SQLite cannot be
    /// opened, or the database schema is not operational.
    ///
    /// # Examples
    /// 从配置打开可复用库门面。 / Open the reusable library facade from configuration.
    ///
    /// <!-- @example 从配置打开可复用库门面。 / Open the reusable library facade from configuration. -->
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

    /// 从已装配依赖构造门面。 / Construct the facade from assembled dependencies.
    ///
    /// <!-- @brief 从已装配依赖构造门面。 / Construct the facade from assembled dependencies. -->
    ///
    /// # Arguments
    /// - `database`: 数据库端口。 / Database port.
    /// <!-- @param database 数据库端口。 / Database port. -->
    /// - `config`: 已验证配置。 / Validated configuration.
    /// <!-- @param config 已验证配置。 / Validated configuration. -->
    ///
    /// # Returns
    /// 应用门面。 / Application facade.
    ///
    /// <!-- @return 应用门面。 / Application facade. -->
    #[must_use]
    pub fn from_parts(database: Box<dyn Database>, config: Config) -> Self {
        Self { database, config }
    }

    /// 执行源程序；完整实现由运行时协调器提供。 / Evaluate source; the runtime coordinator provides the complete implementation.
    ///
    /// <!-- @brief 执行源程序；完整实现由运行时协调器提供。 / Evaluate source; the runtime coordinator provides the complete implementation. -->
    ///
    /// # Arguments
    /// - `source`: DSL 源码。 / DSL source.
    /// <!-- @param source DSL 源码。 / DSL source. -->
    /// - `policy`: 调用能力策略。 / Invocation capability policy.
    /// <!-- @param policy 调用能力策略。 / Invocation capability policy. -->
    ///
    /// # Returns
    /// 类型化值或结构化诊断。 / Typed values or structured diagnostic.
    ///
    /// <!-- @return 类型化值或结构化诊断。 / Typed values or structured diagnostic. -->
    ///
    /// # Errors
    /// 当解析、语义检查、能力检查、持久化或解释失败时，返回结构化诊断。 /
    /// Returns a structured diagnostic when parsing, semantic validation, capability validation,
    /// persistence, or interpretation fails.
    pub fn eval(&mut self, source: &str, policy: InvocationPolicy) -> Result<Vec<Value>> {
        self.eval_preconditioned(source, policy, &[])
    }

    /// 在节点修订前置条件下执行源程序。 / Evaluate source under node revision preconditions.
    ///
    /// <!-- @brief 在节点修订前置条件下执行源程序。 / Evaluate source under node revision preconditions. -->
    ///
    /// # Arguments
    /// - `source`: DSL 源码。 / DSL source.
    /// <!-- @param source DSL 源码。 / DSL source. -->
    /// - `policy`: 调用能力策略。 / Invocation capability policy.
    /// <!-- @param policy 调用能力策略。 / Invocation capability policy. -->
    /// - `preconditions`: 调用方观察到的节点身份与修订。 / Node identities and revisions observed by the caller.
    /// <!-- @param preconditions 调用方观察到的节点身份与修订。 / Node identities and revisions observed by the caller. -->
    ///
    /// # Returns
    /// 条件仍成立时返回类型化值，否则返回 `E_CONFLICT` 且整批不执行。 / Typed values when conditions still hold; otherwise `E_CONFLICT` with no batch execution.
    ///
    /// <!-- @return 条件仍成立时返回类型化值，否则返回 `E_CONFLICT` 且整批不执行。 / Typed values when conditions still hold; otherwise `E_CONFLICT` with no batch execution. -->
    ///
    /// # Errors
    /// 除常规求值错误外，任一节点身份或修订前置条件失效时返回 `E_CONFLICT`。 /
    /// In addition to ordinary evaluation failures, returns `E_CONFLICT` when any node identity or
    /// revision precondition is stale.
    pub fn eval_preconditioned(
        &mut self,
        source: &str,
        policy: InvocationPolicy,
        preconditions: &[NodePrecondition],
    ) -> Result<Vec<Value>> {
        crate::application::runtime::eval(self, source, policy, None, preconditions)
    }

    /// 使用显式文本提供者执行源码。 / Evaluate source with an explicit text provider.
    ///
    /// <!-- @brief 使用显式文本提供者执行源码。 / Evaluate source with an explicit text provider. -->
    ///
    /// # Arguments
    /// - `source`: DSL 源码。 / DSL source.
    /// <!-- @param source DSL 源码。 / DSL source. -->
    /// - `policy`: 调用能力策略。 / Invocation capability policy.
    /// <!-- @param policy 调用能力策略。 / Invocation capability policy. -->
    /// - `provider`: 不持有存储引用的文本提供者。 / Text provider that owns no store reference.
    /// <!-- @param provider 不持有存储引用的文本提供者。 / Text provider that owns no store reference. -->
    ///
    /// # Returns
    /// 提交后可发布的类型化值。 / Typed values publishable after commit.
    ///
    /// <!-- @return 提交后可发布的类型化值。 / Typed values publishable after commit. -->
    ///
    /// # Errors
    /// 当文本提供、解析、检查、持久化或解释失败时，返回结构化诊断。 /
    /// Returns a structured diagnostic when text provision, parsing, validation, persistence, or
    /// interpretation fails.
    pub fn eval_with_provider(
        &mut self,
        source: &str,
        policy: InvocationPolicy,
        provider: &mut dyn TextProvider,
    ) -> Result<Vec<Value>> {
        self.eval_with_provider_preconditioned(source, policy, provider, &[])
    }

    /// 使用文本提供者并在节点修订前置条件下执行源码。 / Evaluate source with a text provider under node revision preconditions.
    ///
    /// <!-- @brief 使用文本提供者并在节点修订前置条件下执行源码。 / Evaluate source with a text provider under node revision preconditions. -->
    ///
    /// # Arguments
    /// - `source`: DSL 源码。 / DSL source.
    /// <!-- @param source DSL 源码。 / DSL source. -->
    /// - `policy`: 调用能力策略。 / Invocation capability policy.
    /// <!-- @param policy 调用能力策略。 / Invocation capability policy. -->
    /// - `provider`: 不持有存储引用的文本提供者。 / Text provider that owns no store reference.
    /// <!-- @param provider 不持有存储引用的文本提供者。 / Text provider that owns no store reference. -->
    /// - `preconditions`: 调用方观察到的节点身份与修订。 / Node identities and revisions observed by the caller.
    /// <!-- @param preconditions 调用方观察到的节点身份与修订。 / Node identities and revisions observed by the caller. -->
    ///
    /// # Returns
    /// 条件仍成立时返回提交后可发布的值，否则返回 `E_CONFLICT` 且整批不执行。 / Values publishable after commit when conditions still hold; otherwise `E_CONFLICT` with no batch execution.
    ///
    /// <!-- @return 条件仍成立时返回提交后可发布的值，否则返回 `E_CONFLICT` 且整批不执行。 / Values publishable after commit when conditions still hold; otherwise `E_CONFLICT` with no batch execution. -->
    ///
    /// # Errors
    /// 除文本提供和常规求值错误外，任一节点身份或修订前置条件失效时返回 `E_CONFLICT`。 /
    /// In addition to text-provider and ordinary evaluation failures, returns `E_CONFLICT` when any
    /// node identity or revision precondition is stale.
    pub fn eval_with_provider_preconditioned(
        &mut self,
        source: &str,
        policy: InvocationPolicy,
        provider: &mut dyn TextProvider,
        preconditions: &[NodePrecondition],
    ) -> Result<Vec<Value>> {
        crate::application::runtime::eval(self, source, policy, Some(provider), preconditions)
    }

    /// 解析并按当前目录快照检查源码但不执行。 / Parse and check source against the current catalog snapshot without executing it.
    ///
    /// <!-- @brief 解析并按当前目录快照检查源码但不执行。 / Parse and check source against the current catalog snapshot without executing it. -->
    ///
    /// # Arguments
    /// - `source`: DSL 源码。 / DSL source.
    /// <!-- @param source DSL 源码。 / DSL source. -->
    /// - `policy`: 调用能力策略。 / Invocation capability policy.
    /// <!-- @param policy 调用能力策略。 / Invocation capability policy. -->
    ///
    /// # Returns
    /// 已检查程序或稳定诊断。 / Checked program or stable diagnostic.
    ///
    /// <!-- @return 已检查程序或稳定诊断。 / Checked program or stable diagnostic. -->
    ///
    /// # Errors
    /// 当源码不完整、语法或语义无效、目录快照不可读，或程序需要策略未允许的能力时，返回诊断。 /
    /// Returns a diagnostic when the source is incomplete, syntactically or semantically invalid,
    /// the catalog snapshot cannot be read, or the program requires a capability denied by policy.
    pub fn check(&self, source: &str, policy: InvocationPolicy) -> Result<CheckedProgram> {
        crate::application::runtime::check(self, source, policy)
    }

    /// 借用生效配置。 / Borrow the effective configuration.
    ///
    /// <!-- @brief 借用生效配置。 / Borrow the effective configuration. -->
    ///
    /// # Returns
    /// 生效配置。 / Effective configuration.
    ///
    /// <!-- @return 生效配置。 / Effective configuration. -->
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 查询持久化适配器的外部变化令牌。 / Query the persistence adapter's external-change token.
    ///
    /// <!-- @brief 查询持久化适配器的外部变化令牌。 / Query the persistence adapter's external-change token. -->
    ///
    /// # Returns
    /// 支持时返回令牌，否则返回 None。 / Token when supported, otherwise None.
    ///
    /// <!-- @return 支持时返回令牌，否则返回 None。 / Token when supported, otherwise None. -->
    ///
    /// # Errors
    /// 当持久化适配器无法读取变化令牌时，返回结构化诊断。 /
    /// Returns a structured diagnostic when the persistence adapter cannot read its change token.
    pub fn change_token(&self) -> Result<Option<u64>> {
        self.database.change_token()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::config::ConfigPaths;

    /// 构造隔离的默认配置。 / Build isolated default configuration.
    ///
    /// <!-- @brief 构造隔离的默认配置。 / Build isolated default configuration. -->
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
