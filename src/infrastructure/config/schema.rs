//! 强类型配置模型 / Typed configuration model.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{ConfigDiagnostic, ConfigPaths};

/// 当前配置模式版本 / Current configuration schema version.
///
/// <!-- @brief 当前配置模式版本 / Current configuration schema version. -->
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// 定义可序列化为稳定字符串字面量的配置枚举 / Defines a configuration enum that
/// serializes to stable string literals.
///
/// <!-- @brief 定义可序列化为稳定字符串字面量的配置枚举 / Defines a configuration enum that serializes to stable string literals. -->
macro_rules! string_enum {
    ($(#[$meta:meta])* $visibility:vis enum $name:ident {
        $($(#[$variant_meta:meta])* $variant:ident => $text:literal),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        $visibility enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            /// 返回稳定配置字面量 / Returns the stable configuration literal.
            ///
            /// <!-- @brief 返回稳定配置字面量 / Returns the stable configuration literal. -->
            /// # Returns
            ///
            /// 稳定字面量 / The stable literal.
            ///
            /// <!-- @return 字面量 / Literal. -->
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }
    };
}

string_enum! {
    /// 终端颜色策略 / Terminal color policy.
    ///
    /// <!-- @brief 终端颜色策略 / Terminal color policy. -->
    pub enum ColorMode {
        /// 自动检测终端颜色能力 / Automatically detects terminal color capabilities.
        ///
        /// <!-- @brief 自动检测终端颜色能力 / Automatically detects terminal color capabilities. -->
        Auto => "auto",
        /// 使用真彩色 / Uses true color.
        ///
        /// <!-- @brief 使用真彩色 / Uses true color. -->
        Truecolor => "truecolor",
        /// 使用 256 色 ANSI 调色板 / Uses the 256-color ANSI palette.
        ///
        /// <!-- @brief 使用 256 色 ANSI 调色板 / Uses the 256-color ANSI palette. -->
        Ansi256 => "ansi256",
        /// 使用 16 色 ANSI 调色板 / Uses the 16-color ANSI palette.
        ///
        /// <!-- @brief 使用 16 色 ANSI 调色板 / Uses the 16-color ANSI palette. -->
        Ansi16 => "ansi16",
        /// 禁用颜色 / Disables color.
        ///
        /// <!-- @brief 禁用颜色 / Disables color. -->
        None => "none",
    }
}
string_enum! {
    /// 字形策略 / Glyph policy.
    ///
    /// <!-- @brief 字形策略 / Glyph policy. -->
    pub enum GlyphMode {
        /// 自动检测字形能力 / Automatically detects glyph capabilities.
        ///
        /// <!-- @brief 自动检测字形能力 / Automatically detects glyph capabilities. -->
        Auto => "auto",
        /// 使用 Unicode 字形 / Uses Unicode glyphs.
        ///
        /// <!-- @brief 使用 Unicode 字形 / Uses Unicode glyphs. -->
        Unicode => "unicode",
        /// 仅使用 ASCII 字形 / Uses ASCII glyphs only.
        ///
        /// <!-- @brief 仅使用 ASCII 字形 / Uses ASCII glyphs only. -->
        Ascii => "ascii",
    }
}
string_enum! {
    /// 默认预览投影 / Default preview projection.
    ///
    /// <!-- @brief 默认预览投影 / Default preview projection. -->
    pub enum PreviewMode {
        /// 显示树投影 / Shows the tree projection.
        ///
        /// <!-- @brief 显示树投影 / Shows the tree projection. -->
        Tree => "tree",
        /// 显示 XML 投影 / Shows the XML projection.
        ///
        /// <!-- @brief 显示 XML 投影 / Shows the XML projection. -->
        Xml => "xml",
        /// 显示内容投影 / Shows the content projection.
        ///
        /// <!-- @brief 显示内容投影 / Shows the content projection. -->
        Content => "content",
        /// 显示元数据投影 / Shows the metadata projection.
        ///
        /// <!-- @brief 显示元数据投影 / Shows the metadata projection. -->
        Metadata => "metadata",
    }
}
string_enum! {
    /// 编辑器提供者 / Editor provider.
    ///
    /// <!-- @brief 编辑器提供者 / Editor provider. -->
    pub enum EditorMode {
        /// 使用内建编辑器 / Uses the built-in editor.
        ///
        /// <!-- @brief 使用内建编辑器 / Uses the built-in editor. -->
        Builtin => "builtin",
        /// 调用配置的外部编辑器 / Invokes the configured external editor.
        ///
        /// <!-- @brief 调用配置的外部编辑器 / Invokes the configured external editor. -->
        External => "external",
    }
}
string_enum! {
    /// 搜索字段 / Search field.
    ///
    /// <!-- @brief 搜索字段 / Search field. -->
    pub enum SearchField {
        /// 仅搜索标题 / Searches titles only.
        ///
        /// <!-- @brief 仅搜索标题 / Searches titles only. -->
        Title => "title",
        /// 仅搜索内容 / Searches content only.
        ///
        /// <!-- @brief 仅搜索内容 / Searches content only. -->
        Content => "content",
        /// 搜索标题与内容 / Searches both titles and content.
        ///
        /// <!-- @brief 搜索标题与内容 / Searches both titles and content. -->
        Mixed => "mixed",
    }
}
string_enum! {
    /// 搜索匹配器 / Search matcher.
    ///
    /// <!-- @brief 搜索匹配器 / Search matcher. -->
    pub enum SearchMatcher {
        /// 使用模糊匹配 / Uses fuzzy matching.
        ///
        /// <!-- @brief 使用模糊匹配 / Uses fuzzy matching. -->
        Fuzzy => "fuzzy",
        /// 使用精确子串匹配 / Uses exact substring matching.
        ///
        /// <!-- @brief 使用精确子串匹配 / Uses exact substring matching. -->
        Exact => "exact",
    }
}
string_enum! {
    /// SQLite 日志模式 / SQLite journal mode.
    ///
    /// <!-- @brief SQLite 日志模式 / SQLite journal mode. -->
    pub enum JournalMode {
        /// 使用预写式日志 / Uses write-ahead logging.
        ///
        /// <!-- @brief 使用预写式日志 / Uses write-ahead logging. -->
        Wal => "wal",
        /// 使用回滚日志并在提交后删除 / Uses a rollback journal deleted after commit.
        ///
        /// <!-- @brief 使用回滚日志并在提交后删除 / Uses a rollback journal deleted after commit. -->
        Delete => "delete",
    }
}

/// 完整语义调色板 / Complete semantic color palette.
///
/// <!-- @brief 完整语义调色板 / Complete semantic color palette. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    /// 表面色 / Surface color.
    ///
    /// <!-- @brief 表面色 / Surface color. -->
    pub surface: String,
    /// 文本色 / Text color.
    ///
    /// <!-- @brief 文本色 / Text color. -->
    pub text: String,
    /// 弱化色 / Muted color.
    ///
    /// <!-- @brief 弱化色 / Muted color. -->
    pub muted: String,
    /// 选中色 / Selection color.
    ///
    /// <!-- @brief 选中色 / Selection color. -->
    pub selection: String,
    /// Fragment 颜色 / Fragment color.
    ///
    /// <!-- @brief Fragment 颜色 / Fragment color. -->
    pub fragment: String,
    /// Prompt 颜色 / Prompt color.
    ///
    /// <!-- @brief Prompt 颜色 / Prompt color. -->
    pub prompt: String,
    /// 标签色 / Tag color.
    ///
    /// <!-- @brief 标签色 / Tag color. -->
    pub tag: String,
    /// 成功色 / Success color.
    ///
    /// <!-- @brief 成功色 / Success color. -->
    pub success: String,
    /// 警告色 / Warning color.
    ///
    /// <!-- @brief 警告色 / Warning color. -->
    pub warning: String,
    /// 错误色 / Error color.
    ///
    /// <!-- @brief 错误色 / Error color. -->
    pub error: String,
}

/// 用户界面配置 / User-interface configuration.
///
/// <!-- @brief 用户界面配置 / User-interface configuration. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UiConfig {
    /// 颜色策略 / Color policy.
    ///
    /// <!-- @brief 颜色策略 / Color policy. -->
    pub color: ColorMode,
    /// 字形策略 / Glyph policy.
    ///
    /// <!-- @brief 字形策略 / Glyph policy. -->
    pub glyphs: GlyphMode,
    /// 主题名 / Theme name.
    ///
    /// <!-- @brief 主题名 / Theme name. -->
    pub theme: String,
    /// 是否启用鼠标 / Whether mouse input is enabled.
    ///
    /// <!-- @brief 是否启用鼠标 / Whether mouse input is enabled. -->
    pub mouse: bool,
    /// 默认预览 / Default preview.
    ///
    /// <!-- @brief 默认预览 / Default preview. -->
    pub preview: PreviewMode,
}

/// 编辑器配置 / Editor configuration.
///
/// <!-- @brief 编辑器配置 / Editor configuration. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditorConfig {
    /// 编辑器模式 / Editor mode.
    ///
    /// <!-- @brief 编辑器模式 / Editor mode. -->
    pub mode: EditorMode,
    /// 直接执行的外部 argv / Direct external argv.
    ///
    /// <!-- @brief 直接执行的外部 argv / Direct external argv. -->
    pub external: Option<Vec<String>>,
}

/// 数据库配置 / Database configuration.
///
/// <!-- @brief 数据库配置 / Database configuration. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DatabaseConfig {
    /// SQLite 文件路径 / SQLite file path.
    ///
    /// <!-- @brief SQLite 文件路径 / SQLite file path. -->
    pub path: PathBuf,
    /// 忙等待毫秒数 / Busy timeout in milliseconds.
    ///
    /// <!-- @brief 忙等待毫秒数 / Busy timeout in milliseconds. -->
    pub busy_timeout_ms: u64,
    /// 是否自动迁移 / Whether automatic migration is enabled.
    ///
    /// <!-- @brief 是否自动迁移 / Whether automatic migration is enabled. -->
    pub auto_migrate: bool,
    /// 迁移前是否备份 / Whether to back up before migration.
    ///
    /// <!-- @brief 迁移前是否备份 / Whether to back up before migration. -->
    pub backup_before_migrate: bool,
    /// 保留的备份数 / Number of backups retained.
    ///
    /// <!-- @brief 保留的备份数 / Number of backups retained. -->
    pub backup_keep: usize,
    /// SQLite 日志模式 / SQLite journal mode.
    ///
    /// <!-- @brief SQLite 日志模式 / SQLite journal mode. -->
    pub journal_mode: JournalMode,
}

/// 搜索配置 / Search configuration.
///
/// <!-- @brief 搜索配置 / Search configuration. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchConfig {
    /// 默认字段 / Default field.
    ///
    /// <!-- @brief 默认字段 / Default field. -->
    pub field: SearchField,
    /// 默认匹配器 / Default matcher.
    ///
    /// <!-- @brief 默认匹配器 / Default matcher. -->
    pub matcher: SearchMatcher,
}

/// 经验证的运行时配置 / Validated runtime configuration.
///
/// <!-- @brief 经验证的运行时配置 / Validated runtime configuration. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Config {
    /// 模式版本 / Schema version.
    ///
    /// <!-- @brief 模式版本 / Schema version. -->
    pub schema_version: u32,
    /// UI 配置 / UI configuration.
    ///
    /// <!-- @brief UI 配置 / UI configuration. -->
    pub ui: UiConfig,
    /// 编辑器配置 / Editor configuration.
    ///
    /// <!-- @brief 编辑器配置 / Editor configuration. -->
    pub editor: EditorConfig,
    /// 数据库配置 / Database configuration.
    ///
    /// <!-- @brief 数据库配置 / Database configuration. -->
    pub database: DatabaseConfig,
    /// 搜索配置 / Search configuration.
    ///
    /// <!-- @brief 搜索配置 / Search configuration. -->
    pub search: SearchConfig,
    /// 自定义完整主题 / Complete custom themes.
    ///
    /// <!-- @brief 自定义完整主题 / Complete custom themes. -->
    pub themes: BTreeMap<String, Theme>,
}

impl Config {
    /// 构造可直接运行的平台默认配置 / Builds usable platform defaults.
    ///
    /// <!-- @brief 构造可直接运行的平台默认配置 / Builds usable platform defaults. -->
    /// # Arguments
    ///
    /// - `paths`: 平台路径 / Platform paths.
    ///
    /// # Returns
    ///
    /// 可直接运行的默认配置 / A ready-to-run default configuration.
    ///
    /// <!-- @param paths 平台路径 / Platform paths. -->
    /// <!-- @return 默认配置 / Default configuration. -->
    pub fn defaults(paths: &ConfigPaths) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            ui: UiConfig {
                color: ColorMode::Auto,
                glyphs: GlyphMode::Auto,
                theme: "dark".into(),
                mouse: true,
                preview: PreviewMode::Tree,
            },
            editor: EditorConfig {
                mode: EditorMode::Builtin,
                external: None,
            },
            database: DatabaseConfig {
                path: paths.database.clone(),
                busy_timeout_ms: 2_000,
                auto_migrate: true,
                backup_before_migrate: true,
                backup_keep: 3,
                journal_mode: JournalMode::Wal,
            },
            search: SearchConfig {
                field: SearchField::Mixed,
                matcher: SearchMatcher::Fuzzy,
            },
            themes: BTreeMap::new(),
        }
    }
}

/// 可逐字段合并的配置层 / Field-wise mergeable configuration layer.
///
/// <!-- @brief 可逐字段合并的配置层 / Field-wise mergeable configuration layer. -->
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverlay {
    /// UI 颜色 / UI color.
    ///
    /// <!-- @brief UI 颜色 / UI color. -->
    pub ui_color: Option<ColorMode>,
    /// UI 字形 / UI glyphs.
    ///
    /// <!-- @brief UI 字形 / UI glyphs. -->
    pub ui_glyphs: Option<GlyphMode>,
    /// UI 主题 / UI theme.
    ///
    /// <!-- @brief UI 主题 / UI theme. -->
    pub ui_theme: Option<String>,
    /// UI 鼠标 / UI mouse.
    ///
    /// <!-- @brief UI 鼠标 / UI mouse. -->
    pub ui_mouse: Option<bool>,
    /// UI 预览 / UI preview.
    ///
    /// <!-- @brief UI 预览 / UI preview. -->
    pub ui_preview: Option<PreviewMode>,
    /// 编辑器模式 / Editor mode.
    ///
    /// <!-- @brief 编辑器模式 / Editor mode. -->
    pub editor_mode: Option<EditorMode>,
    /// 外部编辑器 argv / External editor argv.
    ///
    /// <!-- @brief 外部编辑器 argv / External editor argv. -->
    pub editor_external: Option<Vec<String>>,
    /// 数据库路径 / Database path.
    ///
    /// <!-- @brief 数据库路径 / Database path. -->
    pub database_path: Option<PathBuf>,
    /// 忙等待 / Busy timeout.
    ///
    /// <!-- @brief 忙等待 / Busy timeout. -->
    pub database_busy_timeout_ms: Option<u64>,
    /// 自动迁移 / Automatic migration.
    ///
    /// <!-- @brief 自动迁移 / Automatic migration. -->
    pub database_auto_migrate: Option<bool>,
    /// 迁移备份 / Migration backup.
    ///
    /// <!-- @brief 迁移备份 / Migration backup. -->
    pub database_backup_before_migrate: Option<bool>,
    /// 备份保留数 / Backup retention.
    ///
    /// <!-- @brief 备份保留数 / Backup retention. -->
    pub database_backup_keep: Option<usize>,
    /// 日志模式 / Journal mode.
    ///
    /// <!-- @brief 日志模式 / Journal mode. -->
    pub database_journal_mode: Option<JournalMode>,
    /// 搜索字段 / Search field.
    ///
    /// <!-- @brief 搜索字段 / Search field. -->
    pub search_field: Option<SearchField>,
    /// 搜索匹配器 / Search matcher.
    ///
    /// <!-- @brief 搜索匹配器 / Search matcher. -->
    pub search_matcher: Option<SearchMatcher>,
    /// 主题表 / Theme table.
    ///
    /// <!-- @brief 主题表 / Theme table. -->
    pub themes: BTreeMap<String, Theme>,
}

/// 版本化原始 TOML 配置 / Versioned raw TOML configuration.
///
/// <!-- @brief 版本化原始 TOML 配置 / Versioned raw TOML configuration. -->
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawConfig {
    /// 输入模式版本 / Input schema version.
    ///
    /// <!-- @brief 输入模式版本 / Input schema version. -->
    pub schema_version: Option<u32>,
    /// 原始 UI 表 / Raw UI table.
    ///
    /// <!-- @brief 原始 UI 表 / Raw UI table. -->
    pub ui: Option<RawUi>,
    /// 原始编辑器表 / Raw editor table.
    ///
    /// <!-- @brief 原始编辑器表 / Raw editor table. -->
    pub editor: Option<RawEditor>,
    /// 原始数据库表 / Raw database table.
    ///
    /// <!-- @brief 原始数据库表 / Raw database table. -->
    pub database: Option<RawDatabase>,
    /// 原始搜索表 / Raw search table.
    ///
    /// <!-- @brief 原始搜索表 / Raw search table. -->
    pub search: Option<RawSearch>,
    /// 原始主题表 / Raw theme tables.
    ///
    /// <!-- @brief 原始主题表 / Raw theme tables. -->
    pub themes: Option<BTreeMap<String, Theme>>,
}

/// 原始 UI 层 / Raw UI layer.
///
/// <!-- @brief 原始 UI 层 / Raw UI layer. -->
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawUi {
    /// 颜色 / Color.
    ///
    /// <!-- @brief 颜色 / Color. -->
    pub color: Option<ColorMode>,
    /// 字形 / Glyphs.
    ///
    /// <!-- @brief 字形 / Glyphs. -->
    pub glyphs: Option<GlyphMode>,
    /// 主题 / Theme.
    ///
    /// <!-- @brief 主题 / Theme. -->
    pub theme: Option<String>,
    /// 鼠标 / Mouse.
    ///
    /// <!-- @brief 鼠标 / Mouse. -->
    pub mouse: Option<bool>,
    /// 预览 / Preview.
    ///
    /// <!-- @brief 预览 / Preview. -->
    pub preview: Option<PreviewMode>,
}

/// 原始编辑器层 / Raw editor layer.
///
/// <!-- @brief 原始编辑器层 / Raw editor layer. -->
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawEditor {
    /// 模式 / Mode.
    ///
    /// <!-- @brief 模式 / Mode. -->
    pub mode: Option<EditorMode>,
    /// 外部 argv / External argv.
    ///
    /// <!-- @brief 外部 argv / External argv. -->
    pub external: Option<Vec<String>>,
}

/// 原始数据库层 / Raw database layer.
///
/// <!-- @brief 原始数据库层 / Raw database layer. -->
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawDatabase {
    /// 路径 / Path.
    ///
    /// <!-- @brief 路径 / Path. -->
    pub path: Option<PathBuf>,
    /// 忙等待 / Busy timeout.
    ///
    /// <!-- @brief 忙等待 / Busy timeout. -->
    pub busy_timeout_ms: Option<u64>,
    /// 自动迁移 / Automatic migration.
    ///
    /// <!-- @brief 自动迁移 / Automatic migration. -->
    pub auto_migrate: Option<bool>,
    /// 备份 / Backup.
    ///
    /// <!-- @brief 备份 / Backup. -->
    pub backup_before_migrate: Option<bool>,
    /// 保留数 / Retention count.
    ///
    /// <!-- @brief 保留数 / Retention count. -->
    pub backup_keep: Option<usize>,
    /// 日志模式 / Journal mode.
    ///
    /// <!-- @brief 日志模式 / Journal mode. -->
    pub journal_mode: Option<JournalMode>,
}

/// 原始搜索层 / Raw search layer.
///
/// <!-- @brief 原始搜索层 / Raw search layer. -->
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawSearch {
    /// 字段 / Field.
    ///
    /// <!-- @brief 字段 / Field. -->
    pub field: Option<SearchField>,
    /// 匹配器 / Matcher.
    ///
    /// <!-- @brief 匹配器 / Matcher. -->
    pub matcher: Option<SearchMatcher>,
}

impl RawConfig {
    /// 将原始配置转换为已验证覆盖层 / Converts raw config into a validated overlay.
    ///
    /// <!-- @brief 将原始配置转换为已验证覆盖层 / Converts raw config into a validated overlay. -->
    /// # Returns
    ///
    /// 已验证的配置覆盖层 / The validated configuration overlay.
    ///
    /// # Errors
    ///
    /// 任一配置值违反模式或跨字段约束时返回全部诊断 / Returns all diagnostics when any
    /// configuration value violates schema or cross-field constraints.
    ///
    /// <!-- @return 成功时返回覆盖层，否则返回全部诊断 / Overlay on success, all diagnostics otherwise. -->
    pub fn validate(self) -> Result<ConfigOverlay, Vec<ConfigDiagnostic>> {
        self.validate_at("<memory>")
    }

    /// 在指定源上验证原始配置 / Validates raw configuration at a named source.
    ///
    /// <!-- @brief 在指定源上验证原始配置 / Validates raw configuration at a named source. -->
    /// # Arguments
    ///
    /// - `source`: 配置路径或源名 / Configuration path or source name.
    ///
    /// # Returns
    ///
    /// 已验证的配置覆盖层 / The validated configuration overlay.
    ///
    /// # Errors
    ///
    /// 任一配置值违反模式或跨字段约束时返回带源信息的全部诊断 / Returns all diagnostics,
    /// annotated with the source, when any configuration value violates schema or cross-field
    /// constraints.
    ///
    /// <!-- @param source 配置路径或源名 / Configuration path or source name. -->
    /// <!-- @return 成功时返回覆盖层，否则返回全部诊断 / Overlay on success, all diagnostics otherwise. -->
    pub fn validate_at(self, source: &str) -> Result<ConfigOverlay, Vec<ConfigDiagnostic>> {
        let mut diagnostics = Vec::new();
        let version = self.schema_version.unwrap_or(CURRENT_SCHEMA_VERSION);
        if version > CURRENT_SCHEMA_VERSION {
            diagnostics.push(ConfigDiagnostic::error(
                "E_CONFIG_SCHEMA_NEW",
                format!(
                    "configuration schema is newer than this executable: found={version}, max_supported={CURRENT_SCHEMA_VERSION}, path={source}, app_version={}",
                    env!("CARGO_PKG_VERSION")
                ),
            ).with_source(source).with_suggestion("upgrade Promptr or select a compatible configuration"));
        }

        let editor = self.editor.unwrap_or_default();
        if let Some(argv) = &editor.external {
            if argv.is_empty() {
                diagnostics.push(
                    ConfigDiagnostic::error(
                        "E_CONFIG_EDITOR",
                        "editor.external must contain at least one argv element",
                    )
                    .with_key("editor.external"),
                );
            }
            let placeholders = argv
                .iter()
                .filter(|value| value.as_str() == "{file}")
                .count();
            if placeholders != 1 {
                diagnostics.push(ConfigDiagnostic::error("E_CONFIG_EDITOR", format!("editor.external must contain `{{file}}` as a standalone argv element exactly once, found {placeholders}")).with_key("editor.external"));
            }
        }
        if self
            .database
            .as_ref()
            .and_then(|value| value.busy_timeout_ms)
            == Some(0)
        {
            diagnostics.push(
                ConfigDiagnostic::error(
                    "E_CONFIG_RANGE",
                    "database.busy_timeout_ms must be greater than zero",
                )
                .with_key("database.busy_timeout_ms"),
            );
        }
        if self.database.as_ref().and_then(|value| value.backup_keep) == Some(0) {
            diagnostics.push(
                ConfigDiagnostic::error(
                    "E_CONFIG_RANGE",
                    "database.backup_keep must be greater than zero",
                )
                .with_key("database.backup_keep"),
            );
        }
        if let Some(themes) = &self.themes {
            for (name, theme) in themes {
                for (token, color) in theme.colors() {
                    if !valid_color(color) {
                        diagnostics.push(
                            ConfigDiagnostic::error(
                                "E_CONFIG_COLOR",
                                format!(
                                    "theme `{name}` token `{token}` has invalid color `{color}`"
                                ),
                            )
                            .with_key(format!("themes.{name}.{token}")),
                        );
                    }
                }
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        let ui = self.ui.unwrap_or_default();
        let database = self.database.unwrap_or_default();
        let search = self.search.unwrap_or_default();
        Ok(ConfigOverlay {
            ui_color: ui.color,
            ui_glyphs: ui.glyphs,
            ui_theme: ui.theme,
            ui_mouse: ui.mouse,
            ui_preview: ui.preview,
            editor_mode: editor.mode,
            editor_external: editor.external,
            database_path: database.path,
            database_busy_timeout_ms: database.busy_timeout_ms,
            database_auto_migrate: database.auto_migrate,
            database_backup_before_migrate: database.backup_before_migrate,
            database_backup_keep: database.backup_keep,
            database_journal_mode: database.journal_mode,
            search_field: search.field,
            search_matcher: search.matcher,
            themes: self.themes.unwrap_or_default(),
        })
    }
}

impl Theme {
    fn colors(&self) -> [(&'static str, &str); 10] {
        [
            ("surface", &self.surface),
            ("text", &self.text),
            ("muted", &self.muted),
            ("selection", &self.selection),
            ("fragment", &self.fragment),
            ("prompt", &self.prompt),
            ("tag", &self.tag),
            ("success", &self.success),
            ("warning", &self.warning),
            ("error", &self.error),
        ]
    }
}

fn valid_color(color: &str) -> bool {
    const ANSI: &[&str] = &[
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "gray", "darkgray",
    ];
    (color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit()))
        || ANSI.contains(&color.to_ascii_lowercase().as_str())
}

/// 返回 `config init` 的简洁注释示例 / Returns the concise commented `config init` example.
///
/// <!-- @brief 返回 `config init` 的简洁注释示例 / Returns the concise commented `config init` example. -->
/// # Returns
///
/// 可直接写入配置文件的 TOML 示例 / A TOML example ready to write to a configuration file.
///
/// <!-- @return TOML 示例 / TOML example. -->
pub fn init_example() -> &'static str {
    r#"# Promptr configuration. Every setting below is optional.
schema_version = 1

[ui]
# color = "auto" # auto | truecolor | ansi256 | ansi16 | none
# theme = "dark" # dark | light | monochrome | custom theme name
mouse = true

[editor]
mode = "builtin"
# To use an external editor, set mode = "external" and uncomment:
# external = ["nvim", "{file}"]

[database]
auto_migrate = true
backup_before_migrate = true

[search]
field = "mixed"
matcher = "fuzzy"
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_is_the_editor_default() {
        let paths = ConfigPaths {
            user_config: "c".into(),
            database: "d".into(),
            state_dir: "s".into(),
            cache_dir: "x".into(),
            backup_dir: "b".into(),
        };
        assert_eq!(Config::defaults(&paths).editor.mode, EditorMode::Builtin);
    }

    #[test]
    fn external_editor_requires_exactly_one_placeholder() {
        let raw = RawConfig {
            editor: Some(RawEditor {
                mode: Some(EditorMode::External),
                external: Some(vec!["nvim".into()]),
            }),
            ..RawConfig::default()
        };
        assert!(
            raw.validate()
                .unwrap_err()
                .iter()
                .any(|value| value.code == "E_CONFIG_EDITOR")
        );
    }

    #[test]
    fn file_placeholder_must_be_a_standalone_argument() {
        let raw = RawConfig {
            editor: Some(RawEditor {
                mode: Some(EditorMode::External),
                external: Some(vec!["nvim".into(), "--file={file}".into()]),
            }),
            ..RawConfig::default()
        };
        assert!(raw.validate().is_err());
    }
}
