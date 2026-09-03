//! 强类型配置模型 / Typed configuration model.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{ConfigDiagnostic, ConfigPaths};

/// @brief 当前配置模式版本 / Current configuration schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

macro_rules! string_enum {
    ($(#[$meta:meta])* $visibility:vis enum $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        $visibility enum $name { $($variant),+ }

        impl $name {
            /// @brief 返回稳定配置字面量 / Returns the stable configuration literal.
            /// @return 字面量 / Literal.
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }
    };
}

string_enum! {
    /// @brief 终端颜色策略 / Terminal color policy.
    pub enum ColorMode { Auto => "auto", Truecolor => "truecolor", Ansi256 => "ansi256", Ansi16 => "ansi16", None => "none" }
}
string_enum! {
    /// @brief 字形策略 / Glyph policy.
    pub enum GlyphMode { Auto => "auto", Unicode => "unicode", Ascii => "ascii" }
}
string_enum! {
    /// @brief 默认预览投影 / Default preview projection.
    pub enum PreviewMode { Tree => "tree", Xml => "xml", Content => "content", Metadata => "metadata" }
}
string_enum! {
    /// @brief 编辑器提供者 / Editor provider.
    pub enum EditorMode { Builtin => "builtin", External => "external" }
}
string_enum! {
    /// @brief 搜索字段 / Search field.
    pub enum SearchField { Title => "title", Content => "content", Mixed => "mixed" }
}
string_enum! {
    /// @brief 搜索匹配器 / Search matcher.
    pub enum SearchMatcher { Fuzzy => "fuzzy", Exact => "exact" }
}
string_enum! {
    /// @brief SQLite 日志模式 / SQLite journal mode.
    pub enum JournalMode { Wal => "wal", Delete => "delete" }
}

/// @brief 完整语义调色板 / Complete semantic color palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    /// @brief 表面色 / Surface color.
    pub surface: String,
    /// @brief 文本色 / Text color.
    pub text: String,
    /// @brief 弱化色 / Muted color.
    pub muted: String,
    /// @brief 选中色 / Selection color.
    pub selection: String,
    /// @brief Fragment 颜色 / Fragment color.
    pub fragment: String,
    /// @brief Prompt 颜色 / Prompt color.
    pub prompt: String,
    /// @brief 标签色 / Tag color.
    pub tag: String,
    /// @brief 成功色 / Success color.
    pub success: String,
    /// @brief 警告色 / Warning color.
    pub warning: String,
    /// @brief 错误色 / Error color.
    pub error: String,
}

/// @brief 用户界面配置 / User-interface configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UiConfig {
    /// @brief 颜色策略 / Color policy.
    pub color: ColorMode,
    /// @brief 字形策略 / Glyph policy.
    pub glyphs: GlyphMode,
    /// @brief 主题名 / Theme name.
    pub theme: String,
    /// @brief 是否启用鼠标 / Whether mouse input is enabled.
    pub mouse: bool,
    /// @brief 默认预览 / Default preview.
    pub preview: PreviewMode,
}

/// @brief 编辑器配置 / Editor configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditorConfig {
    /// @brief 编辑器模式 / Editor mode.
    pub mode: EditorMode,
    /// @brief 直接执行的外部 argv / Direct external argv.
    pub external: Option<Vec<String>>,
}

/// @brief 数据库配置 / Database configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DatabaseConfig {
    /// @brief SQLite 文件路径 / SQLite file path.
    pub path: PathBuf,
    /// @brief 忙等待毫秒数 / Busy timeout in milliseconds.
    pub busy_timeout_ms: u64,
    /// @brief 是否自动迁移 / Whether automatic migration is enabled.
    pub auto_migrate: bool,
    /// @brief 迁移前是否备份 / Whether to back up before migration.
    pub backup_before_migrate: bool,
    /// @brief 保留的备份数 / Number of backups retained.
    pub backup_keep: usize,
    /// @brief SQLite 日志模式 / SQLite journal mode.
    pub journal_mode: JournalMode,
}

/// @brief 搜索配置 / Search configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchConfig {
    /// @brief 默认字段 / Default field.
    pub field: SearchField,
    /// @brief 默认匹配器 / Default matcher.
    pub matcher: SearchMatcher,
}

/// @brief 经验证的运行时配置 / Validated runtime configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Config {
    /// @brief 模式版本 / Schema version.
    pub schema_version: u32,
    /// @brief UI 配置 / UI configuration.
    pub ui: UiConfig,
    /// @brief 编辑器配置 / Editor configuration.
    pub editor: EditorConfig,
    /// @brief 数据库配置 / Database configuration.
    pub database: DatabaseConfig,
    /// @brief 搜索配置 / Search configuration.
    pub search: SearchConfig,
    /// @brief 自定义完整主题 / Complete custom themes.
    pub themes: BTreeMap<String, Theme>,
}

impl Config {
    /// @brief 构造可直接运行的平台默认配置 / Builds usable platform defaults.
    /// @param paths 平台路径 / Platform paths.
    /// @return 默认配置 / Default configuration.
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

/// @brief 可逐字段合并的配置层 / Field-wise mergeable configuration layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverlay {
    /// @brief UI 颜色 / UI color.
    pub ui_color: Option<ColorMode>,
    /// @brief UI 字形 / UI glyphs.
    pub ui_glyphs: Option<GlyphMode>,
    /// @brief UI 主题 / UI theme.
    pub ui_theme: Option<String>,
    /// @brief UI 鼠标 / UI mouse.
    pub ui_mouse: Option<bool>,
    /// @brief UI 预览 / UI preview.
    pub ui_preview: Option<PreviewMode>,
    /// @brief 编辑器模式 / Editor mode.
    pub editor_mode: Option<EditorMode>,
    /// @brief 外部编辑器 argv / External editor argv.
    pub editor_external: Option<Vec<String>>,
    /// @brief 数据库路径 / Database path.
    pub database_path: Option<PathBuf>,
    /// @brief 忙等待 / Busy timeout.
    pub database_busy_timeout_ms: Option<u64>,
    /// @brief 自动迁移 / Automatic migration.
    pub database_auto_migrate: Option<bool>,
    /// @brief 迁移备份 / Migration backup.
    pub database_backup_before_migrate: Option<bool>,
    /// @brief 备份保留数 / Backup retention.
    pub database_backup_keep: Option<usize>,
    /// @brief 日志模式 / Journal mode.
    pub database_journal_mode: Option<JournalMode>,
    /// @brief 搜索字段 / Search field.
    pub search_field: Option<SearchField>,
    /// @brief 搜索匹配器 / Search matcher.
    pub search_matcher: Option<SearchMatcher>,
    /// @brief 主题表 / Theme table.
    pub themes: BTreeMap<String, Theme>,
}

/// @brief 版本化原始 TOML 配置 / Versioned raw TOML configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawConfig {
    /// @brief 输入模式版本 / Input schema version.
    pub schema_version: Option<u32>,
    /// @brief 原始 UI 表 / Raw UI table.
    pub ui: Option<RawUi>,
    /// @brief 原始编辑器表 / Raw editor table.
    pub editor: Option<RawEditor>,
    /// @brief 原始数据库表 / Raw database table.
    pub database: Option<RawDatabase>,
    /// @brief 原始搜索表 / Raw search table.
    pub search: Option<RawSearch>,
    /// @brief 原始主题表 / Raw theme tables.
    pub themes: Option<BTreeMap<String, Theme>>,
}

/// @brief 原始 UI 层 / Raw UI layer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawUi {
    /// @brief 颜色 / Color.
    pub color: Option<ColorMode>,
    /// @brief 字形 / Glyphs.
    pub glyphs: Option<GlyphMode>,
    /// @brief 主题 / Theme.
    pub theme: Option<String>,
    /// @brief 鼠标 / Mouse.
    pub mouse: Option<bool>,
    /// @brief 预览 / Preview.
    pub preview: Option<PreviewMode>,
}

/// @brief 原始编辑器层 / Raw editor layer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawEditor {
    /// @brief 模式 / Mode.
    pub mode: Option<EditorMode>,
    /// @brief 外部 argv / External argv.
    pub external: Option<Vec<String>>,
}

/// @brief 原始数据库层 / Raw database layer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawDatabase {
    /// @brief 路径 / Path.
    pub path: Option<PathBuf>,
    /// @brief 忙等待 / Busy timeout.
    pub busy_timeout_ms: Option<u64>,
    /// @brief 自动迁移 / Automatic migration.
    pub auto_migrate: Option<bool>,
    /// @brief 备份 / Backup.
    pub backup_before_migrate: Option<bool>,
    /// @brief 保留数 / Retention count.
    pub backup_keep: Option<usize>,
    /// @brief 日志模式 / Journal mode.
    pub journal_mode: Option<JournalMode>,
}

/// @brief 原始搜索层 / Raw search layer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawSearch {
    /// @brief 字段 / Field.
    pub field: Option<SearchField>,
    /// @brief 匹配器 / Matcher.
    pub matcher: Option<SearchMatcher>,
}

impl RawConfig {
    /// @brief 将原始配置转换为已验证覆盖层 / Converts raw config into a validated overlay.
    /// @return 成功时返回覆盖层，否则返回全部诊断 / Overlay on success, all diagnostics otherwise.
    pub fn validate(self) -> Result<ConfigOverlay, Vec<ConfigDiagnostic>> {
        let mut diagnostics = Vec::new();
        let version = self.schema_version.unwrap_or(CURRENT_SCHEMA_VERSION);
        if version > CURRENT_SCHEMA_VERSION {
            diagnostics.push(ConfigDiagnostic::error(
                "E_CONFIG_NEWER",
                format!("configuration schema {version} requires a newer Promptr; this executable supports schema {CURRENT_SCHEMA_VERSION}"),
            ).with_suggestion("upgrade Promptr or select a schema-1 configuration"));
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

/// @brief 返回 `config init` 的简洁注释示例 / Returns the concise commented `config init` example.
/// @return TOML 示例 / TOML example.
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
