//! 无损解析与分层加载 / Lossless parsing and layered loading.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use toml_edit::{DocumentMut, InlineTable, Item, Value};

use super::{
    ColorMode, Config, ConfigDiagnostic, ConfigOverlay, ConfigPaths, EditorMode, GlyphMode,
    RawConfig, SearchField,
};

/// 配置值的来源类别 / Kind of configuration value source.
///
/// <!-- @brief 配置值的来源类别 / Kind of configuration value source. -->
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum ConfigSource {
    /// 内建默认 / Built-in default.
    ///
    /// <!-- @brief 内建默认 / Built-in default. -->
    Builtin,
    /// 平台用户文件 / Platform user file.
    ///
    /// <!-- @brief 平台用户文件 / Platform user file. -->
    UserFile(PathBuf),
    /// 显式选择的文件 / Explicitly selected file.
    ///
    /// <!-- @brief 显式选择的文件 / Explicitly selected file. -->
    ExplicitFile(PathBuf),
    /// 环境变量 / Environment variable.
    ///
    /// <!-- @brief 环境变量 / Environment variable. -->
    Environment(String),
    /// CLI 选项 / CLI option.
    ///
    /// <!-- @brief CLI 选项 / CLI option. -->
    Cli(String),
}

/// 生效值的来源记录 / Provenance record for an effective value.
///
/// <!-- @brief 生效值的来源记录 / Provenance record for an effective value. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    /// 当前来源 / Winning source.
    ///
    /// <!-- @brief 当前来源 / Winning source. -->
    pub source: ConfigSource,
    /// 被覆盖的旧来源，按覆盖顺序排列 / Replaced sources in overlay order.
    ///
    /// <!-- @brief 被覆盖的旧来源，按覆盖顺序排列 / Replaced sources in overlay order. -->
    pub overridden: Vec<ConfigSource>,
}

/// 已解析但未合并的无损文档 / Parsed, unmerged lossless document.
///
/// <!-- @brief 已解析但未合并的无损文档 / Parsed, unmerged lossless document. -->
#[derive(Debug, Clone)]
pub struct ParsedConfig {
    /// 保留注释和排版的 TOML AST / TOML AST preserving comments and formatting.
    ///
    /// <!-- @brief 保留注释和排版的 TOML AST / TOML AST preserving comments and formatting. -->
    pub document: DocumentMut,
    /// 版本化原始结构 / Versioned raw structure.
    ///
    /// <!-- @brief 版本化原始结构 / Versioned raw structure. -->
    pub raw: RawConfig,
    /// 文件中的原始模式版本 / Original schema version in the file.
    ///
    /// <!-- @brief 文件中的原始模式版本 / Original schema version in the file. -->
    pub input_schema_version: u32,
    /// 未知键等结构诊断 / Structural diagnostics such as unknown keys.
    ///
    /// <!-- @brief 未知键等结构诊断 / Structural diagnostics such as unknown keys. -->
    pub diagnostics: Vec<ConfigDiagnostic>,
}

impl ParsedConfig {
    /// 无损解析 TOML 文本 / Parses TOML text losslessly.
    ///
    /// <!-- @brief 无损解析 TOML 文本 / Parses TOML text losslessly. -->
    /// # Arguments
    ///
    /// - `text`: TOML 文本 / TOML text.
    /// - `source`: 用于诊断的源名 / Source name used by diagnostics.
    ///
    /// # Returns
    ///
    /// 保留排版的解析文档 / A parsed document that preserves formatting.
    ///
    /// # Errors
    ///
    /// TOML 语法、模式版本或结构无效时返回全部诊断 / Returns all diagnostics when the
    /// TOML syntax, schema version, or structure is invalid.
    ///
    /// <!-- @param text TOML 文本 / TOML text. -->
    /// <!-- @param source 用于诊断的源名 / Source name used by diagnostics. -->
    /// <!-- @return 解析文档或语法诊断 / Parsed document or syntax diagnostic. -->
    pub fn parse(text: &str, source: impl Into<String>) -> Result<Self, Vec<ConfigDiagnostic>> {
        let source = source.into();
        let document = text.parse::<DocumentMut>().map_err(|error| {
            vec![
                ConfigDiagnostic::error("E_CONFIG_PARSE", error.to_string())
                    .with_source(source.clone()),
            ]
        })?;
        let input_schema_version = match document.get("schema_version") {
            Some(item) => item
                .as_integer()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    vec![
                        ConfigDiagnostic::error(
                            "E_CONFIG_TYPE",
                            "schema_version must be a non-negative 32-bit integer",
                        )
                        .with_key("schema_version")
                        .with_source(source.clone()),
                    ]
                })?,
            None => super::CURRENT_SCHEMA_VERSION,
        };
        if input_schema_version > super::CURRENT_SCHEMA_VERSION {
            return Err(RawConfig {
                schema_version: Some(input_schema_version),
                ..RawConfig::default()
            }
            .validate_at(&source)
            .expect_err("newer schema is incompatible"));
        }
        let mut effective_document = document.clone();
        if input_schema_version < super::CURRENT_SCHEMA_VERSION {
            super::migration::upgrade_document(&mut effective_document, input_schema_version)?;
        }
        let raw = toml_edit::de::from_str::<RawConfig>(&effective_document.to_string()).map_err(
            |error| {
                vec![
                    ConfigDiagnostic::error("E_CONFIG_TYPE", error.to_string())
                        .with_source(source.clone()),
                ]
            },
        )?;
        let diagnostics = unknown_key_diagnostics(&document, text, &source);
        Ok(Self {
            document,
            raw,
            input_schema_version,
            diagnostics,
        })
    }
}

/// 单个有来源的已类型化覆盖层 / One sourced typed overlay layer.
///
/// <!-- @brief 单个有来源的已类型化覆盖层 / One sourced typed overlay layer. -->
#[derive(Debug, Clone)]
pub struct ConfigLayer {
    /// 层的来源 / Layer source.
    ///
    /// <!-- @brief 层的来源 / Layer source. -->
    pub source: ConfigSource,
    /// 逐字段覆盖 / Field-wise overlay.
    ///
    /// <!-- @brief 逐字段覆盖 / Field-wise overlay. -->
    pub overlay: ConfigOverlay,
}

/// 可注入环境变量集 / Injectable environment variable set.
///
/// <!-- @brief 可注入环境变量集 / Injectable environment variable set. -->
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// 环境变量名值表 / Environment name-value map.
    ///
    /// <!-- @brief 环境变量名值表 / Environment name-value map. -->
    pub values: BTreeMap<String, OsString>,
}

impl Environment {
    /// 捕获当前进程环境 / Captures the current process environment.
    ///
    /// <!-- @brief 捕获当前进程环境 / Captures the current process environment. -->
    /// # Returns
    ///
    /// 可测试的环境快照 / A testable environment snapshot.
    ///
    /// <!-- @return 可测试的环境快照 / A testable environment snapshot. -->
    pub fn current() -> Self {
        const KNOWN: &[&str] = &[
            "PROMPTR_CONFIG",
            "PROMPTR_DATABASE",
            "PROMPTR_COLOR",
            "PROMPTR_GLYPHS",
            "PROMPTR_EDITOR_MODE",
            "PROMPTR_SEARCH_FIELD",
        ];
        Self {
            values: KNOWN
                .iter()
                .filter_map(|name| std::env::var_os(name).map(|value| ((*name).into(), value)))
                .collect(),
        }
    }
}

/// 配置加载请求 / Configuration loading request.
///
/// <!-- @brief 配置加载请求 / Configuration loading request. -->
#[derive(Debug, Clone, Default)]
pub struct LoadRequest {
    /// 是否禁用所有文件层 / Whether all file layers are disabled.
    ///
    /// <!-- @brief 是否禁用所有文件层 / Whether all file layers are disabled. -->
    pub no_config: bool,
    /// 可选用户配置路径覆盖 / Optional user configuration path override.
    ///
    /// <!-- @brief 可选用户配置路径覆盖 / Optional user configuration path override. -->
    pub user_config: Option<PathBuf>,
    /// `--config` 或 `PROMPTR_CONFIG` 选中的显式文件 / Explicit file selected by `--config` or `PROMPTR_CONFIG`.
    ///
    /// <!-- @brief `--config` 或 `PROMPTR_CONFIG` 选中的显式文件 / Explicit file selected by `--config` or `PROMPTR_CONFIG`. -->
    pub explicit_config: Option<PathBuf>,
    /// 环境覆盖 / Environment overrides.
    ///
    /// <!-- @brief 环境覆盖 / Environment overrides. -->
    pub environment: Environment,
    /// CLI 覆盖 / CLI overrides.
    ///
    /// <!-- @brief CLI 覆盖 / CLI overrides. -->
    pub cli: ConfigOverlay,
}

/// 加载成功的配置及来源 / Successfully loaded configuration and provenance.
///
/// <!-- @brief 加载成功的配置及来源 / Successfully loaded configuration and provenance. -->
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    /// 经验证配置 / Validated configuration.
    ///
    /// <!-- @brief 经验证配置 / Validated configuration. -->
    pub config: Config,
    /// 按点分键索引的来源 / Provenance indexed by dotted key.
    ///
    /// <!-- @brief 按点分键索引的来源 / Provenance indexed by dotted key. -->
    pub provenance: BTreeMap<String, Provenance>,
    /// 非阻止诊断 / Non-blocking diagnostics.
    ///
    /// <!-- @brief 非阻止诊断 / Non-blocking diagnostics. -->
    pub diagnostics: Vec<ConfigDiagnostic>,
}

/// `config show --effective` 的稳定 DTO / Stable DTO for `config show --effective`.
///
/// <!-- @brief `config show --effective` 的稳定 DTO / Stable DTO for `config show --effective`. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectiveConfig {
    /// 按键排序的生效项 / Effective entries sorted by key.
    ///
    /// <!-- @brief 按键排序的生效项 / Effective entries sorted by key. -->
    pub entries: Vec<EffectiveEntry>,
}

/// 单个生效配置项 / One effective configuration entry.
///
/// <!-- @brief 单个生效配置项 / One effective configuration entry. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectiveEntry {
    /// 点分键 / Dotted key.
    ///
    /// <!-- @brief 点分键 / Dotted key. -->
    pub key: String,
    /// TOML 风格值 / TOML-style value.
    ///
    /// <!-- @brief TOML 风格值 / TOML-style value. -->
    pub value: String,
    /// 来源 / Provenance.
    ///
    /// <!-- @brief 来源 / Provenance. -->
    pub provenance: Provenance,
}

/// `config explain` 结果 DTO / Result DTO for `config explain`.
///
/// <!-- @brief `config explain` 结果 DTO / Result DTO for `config explain`. -->
pub type ExplainValue = EffectiveEntry;

impl LoadedConfig {
    /// 生成全部生效值 DTO / Builds the complete effective-value DTO.
    ///
    /// <!-- @brief 生成全部生效值 DTO / Builds the complete effective-value DTO. -->
    /// # Returns
    ///
    /// 稳定排序的值和来源 / Stably sorted values and provenance.
    ///
    /// <!-- @return 稳定排序的值和来源 / Stably sorted values and provenance. -->
    pub fn effective(&self) -> EffectiveConfig {
        let values = config_values(&self.config);
        let entries = values
            .into_iter()
            .map(|(key, value)| EffectiveEntry {
                provenance: self
                    .provenance
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(builtin_provenance),
                key,
                value,
            })
            .collect();
        EffectiveConfig { entries }
    }

    /// 解释单个点分键 / Explains one dotted key.
    ///
    /// <!-- @brief 解释单个点分键 / Explains one dotted key. -->
    /// # Arguments
    ///
    /// - `key`: 点分键 / Dotted key.
    ///
    /// # Returns
    ///
    /// 值和来源，未知键返回 [`None`] / Value and provenance, or [`None`] for an unknown key.
    ///
    /// <!-- @param key 点分键 / Dotted key. -->
    /// <!-- @return 值和来源，未知键返回 None / Value and provenance, or None for an unknown key. -->
    pub fn explain(&self, key: &str) -> Option<ExplainValue> {
        self.effective()
            .entries
            .into_iter()
            .find(|entry| entry.key == key)
    }
}

/// 无全局单例的配置加载器 / Configuration loader without a global singleton.
///
/// <!-- @brief 无全局单例的配置加载器 / Configuration loader without a global singleton. -->
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    /// 平台目录 / Platform directories.
    ///
    /// <!-- @brief 平台目录 / Platform directories. -->
    pub paths: ConfigPaths,
}

impl ConfigLoader {
    /// 创建加载器 / Creates a loader.
    ///
    /// <!-- @brief 创建加载器 / Creates a loader. -->
    /// # Arguments
    ///
    /// - `paths`: 平台目录 / Platform directories.
    ///
    /// # Returns
    ///
    /// 新的加载器 / A new loader.
    ///
    /// <!-- @param paths 平台目录 / Platform directories. -->
    /// <!-- @return 加载器 / Loader. -->
    pub fn new(paths: ConfigPaths) -> Self {
        Self { paths }
    }

    /// 按 defaults-user-explicit-env-CLI 顺序加载 / Loads in defaults-user-explicit-env-CLI order.
    ///
    /// <!-- @brief 按 defaults-user-explicit-env-CLI 顺序加载 / Loads in defaults-user-explicit-env-CLI order. -->
    /// # Arguments
    ///
    /// - `request`: 加载请求 / Load request.
    ///
    /// # Returns
    ///
    /// 已验证的配置及其来源 / The validated configuration and its provenance.
    ///
    /// # Errors
    ///
    /// 文件不可读、配置不可解析或合并后的配置无效时返回全部诊断 / Returns all diagnostics
    /// when a file cannot be read, configuration cannot be parsed, or the merged configuration is
    /// invalid.
    ///
    /// <!-- @param request 加载请求 / Load request. -->
    /// <!-- @return 已验证配置或诊断集 / Validated configuration or diagnostics. -->
    pub fn load(&self, request: LoadRequest) -> Result<LoadedConfig, Vec<ConfigDiagnostic>> {
        let mut layers = Vec::new();
        let mut diagnostics = Vec::new();
        if !request.no_config {
            let user_path = request
                .user_config
                .unwrap_or_else(|| self.paths.user_config.clone());
            if user_path.exists() {
                layers.push(read_layer(
                    &user_path,
                    ConfigSource::UserFile(user_path.clone()),
                    &mut diagnostics,
                )?);
            }
            let explicit = request.explicit_config.or_else(|| {
                request
                    .environment
                    .values
                    .get("PROMPTR_CONFIG")
                    .map(PathBuf::from)
            });
            if let Some(path) = explicit {
                layers.push(read_layer(
                    &path,
                    ConfigSource::ExplicitFile(path.clone()),
                    &mut diagnostics,
                )?);
            }
        }
        layers.extend(environment_layers(&request.environment, &mut diagnostics));
        layers.push(ConfigLayer {
            source: ConfigSource::Cli("command line".into()),
            overlay: request.cli,
        });
        if diagnostics
            .iter()
            .any(|value| value.severity == super::DiagnosticSeverity::Error)
        {
            return Err(diagnostics);
        }
        let mut loaded = self.merge(layers)?;
        loaded.diagnostics = diagnostics;
        Ok(loaded)
    }

    /// 合并已类型化层，便于测试和嵌入 / Merges typed layers for tests and embedding.
    ///
    /// <!-- @brief 合并已类型化层，便于测试和嵌入 / Merges typed layers for tests and embedding. -->
    /// # Arguments
    ///
    /// - `layers`: 按低到高优先级排列的层 / Layers ordered from low to high precedence.
    ///
    /// # Returns
    ///
    /// 已验证的合并配置 / The validated merged configuration.
    ///
    /// # Errors
    ///
    /// 合并后的配置违反跨字段约束时返回全部诊断 / Returns all diagnostics when the merged
    /// configuration violates cross-field constraints.
    ///
    /// <!-- @param layers 按低到高优先级排列的层 / Layers ordered from low to high precedence. -->
    /// <!-- @return 已验证配置 / Validated configuration. -->
    pub fn merge(&self, layers: Vec<ConfigLayer>) -> Result<LoadedConfig, Vec<ConfigDiagnostic>> {
        let mut config = Config::defaults(&self.paths);
        let mut provenance = BTreeMap::new();
        for key in config_values(&config).keys() {
            provenance.insert(key.clone(), builtin_provenance());
        }
        for layer in layers {
            apply_layer(&mut config, &mut provenance, layer);
        }
        validate_effective(&config)?;
        Ok(LoadedConfig {
            config,
            provenance,
            diagnostics: Vec::new(),
        })
    }
}

fn read_layer(
    path: &Path,
    source: ConfigSource,
    diagnostics: &mut Vec<ConfigDiagnostic>,
) -> Result<ConfigLayer, Vec<ConfigDiagnostic>> {
    let text = fs::read_to_string(path).map_err(|error| {
        vec![
            ConfigDiagnostic::error(
                "E_CONFIG_IO",
                format!("cannot read `{}`: {error}", path.display()),
            )
            .with_source(path.display().to_string()),
        ]
    })?;
    let parsed = ParsedConfig::parse(&text, path.display().to_string())?;
    diagnostics.extend(parsed.diagnostics);
    let input_version = parsed.input_schema_version;
    if input_version < super::CURRENT_SCHEMA_VERSION {
        diagnostics.push(
            ConfigDiagnostic::warning(
                "W_CONFIG_MIGRATION_AVAILABLE",
                format!(
                    "`{}` can be explicitly migrated to schema {}",
                    path.display(),
                    super::CURRENT_SCHEMA_VERSION
                ),
            )
            .with_suggestion("run `promptr config migrate` when convenient"),
        );
    }
    let overlay = parsed.raw.validate_at(&path.display().to_string())?;
    Ok(ConfigLayer { source, overlay })
}

fn environment_layers(
    environment: &Environment,
    diagnostics: &mut Vec<ConfigDiagnostic>,
) -> Vec<ConfigLayer> {
    let mut layers = Vec::new();
    if let Some(value) = environment.values.get("PROMPTR_DATABASE") {
        let overlay = ConfigOverlay {
            database_path: Some(PathBuf::from(value)),
            ..ConfigOverlay::default()
        };
        layers.push(ConfigLayer {
            source: ConfigSource::Environment("PROMPTR_DATABASE".into()),
            overlay,
        });
    }
    add_enum_environment(
        environment,
        "PROMPTR_COLOR",
        &mut layers,
        diagnostics,
        |text, overlay| overlay.ui_color = parse_color(text).ok(),
    );
    add_enum_environment(
        environment,
        "PROMPTR_GLYPHS",
        &mut layers,
        diagnostics,
        |text, overlay| overlay.ui_glyphs = parse_glyphs(text).ok(),
    );
    add_enum_environment(
        environment,
        "PROMPTR_EDITOR_MODE",
        &mut layers,
        diagnostics,
        |text, overlay| overlay.editor_mode = parse_editor(text).ok(),
    );
    add_enum_environment(
        environment,
        "PROMPTR_SEARCH_FIELD",
        &mut layers,
        diagnostics,
        |text, overlay| overlay.search_field = parse_field(text).ok(),
    );
    layers
}

fn add_enum_environment(
    environment: &Environment,
    name: &'static str,
    layers: &mut Vec<ConfigLayer>,
    diagnostics: &mut Vec<ConfigDiagnostic>,
    assign: impl FnOnce(&str, &mut ConfigOverlay),
) {
    let Some(value) = environment.values.get(name) else {
        return;
    };
    let Some(text) = value.to_str() else {
        diagnostics.push(
            ConfigDiagnostic::error(
                "E_CONFIG_ENV_UNICODE",
                format!("{name} must be valid Unicode because it selects an enum value"),
            )
            .with_source(name),
        );
        return;
    };
    let mut overlay = ConfigOverlay::default();
    assign(text, &mut overlay);
    let is_valid = match name {
        "PROMPTR_COLOR" => parse_color(text).is_ok(),
        "PROMPTR_GLYPHS" => parse_glyphs(text).is_ok(),
        "PROMPTR_EDITOR_MODE" => parse_editor(text).is_ok(),
        "PROMPTR_SEARCH_FIELD" => parse_field(text).is_ok(),
        _ => false,
    };
    if is_valid {
        layers.push(ConfigLayer {
            source: ConfigSource::Environment(name.into()),
            overlay,
        });
    } else {
        diagnostics.push(
            ConfigDiagnostic::error(
                "E_CONFIG_ENV",
                format!("{name} has invalid enum value `{text}`"),
            )
            .with_source(name),
        );
    }
}

fn parse_color(value: &str) -> Result<ColorMode, &'static str> {
    match value {
        "auto" => Ok(ColorMode::Auto),
        "truecolor" => Ok(ColorMode::Truecolor),
        "ansi256" => Ok(ColorMode::Ansi256),
        "ansi16" => Ok(ColorMode::Ansi16),
        "none" => Ok(ColorMode::None),
        _ => Err("auto|truecolor|ansi256|ansi16|none"),
    }
}
fn parse_glyphs(value: &str) -> Result<GlyphMode, &'static str> {
    match value {
        "auto" => Ok(GlyphMode::Auto),
        "unicode" => Ok(GlyphMode::Unicode),
        "ascii" => Ok(GlyphMode::Ascii),
        _ => Err("auto|unicode|ascii"),
    }
}
fn parse_editor(value: &str) -> Result<EditorMode, &'static str> {
    match value {
        "builtin" => Ok(EditorMode::Builtin),
        "external" => Ok(EditorMode::External),
        _ => Err("builtin|external"),
    }
}
fn parse_field(value: &str) -> Result<SearchField, &'static str> {
    match value {
        "title" => Ok(SearchField::Title),
        "content" => Ok(SearchField::Content),
        "mixed" => Ok(SearchField::Mixed),
        _ => Err("title|content|mixed"),
    }
}

fn set_provenance(map: &mut BTreeMap<String, Provenance>, key: &str, source: &ConfigSource) {
    let previous = map.insert(
        key.into(),
        Provenance {
            source: source.clone(),
            overridden: Vec::new(),
        },
    );
    if let Some(previous) = previous {
        let current = map.get_mut(key).expect("just inserted provenance");
        current.overridden.extend(previous.overridden);
        current.overridden.push(previous.source);
    }
}

fn apply_layer(
    config: &mut Config,
    provenance: &mut BTreeMap<String, Provenance>,
    layer: ConfigLayer,
) {
    macro_rules! apply {
        ($option:expr, $target:expr, $key:literal) => {
            if let Some(value) = $option {
                $target = value;
                set_provenance(provenance, $key, &layer.source);
            }
        };
    }
    let overlay = layer.overlay;
    apply!(overlay.ui_color, config.ui.color, "ui.color");
    apply!(overlay.ui_glyphs, config.ui.glyphs, "ui.glyphs");
    apply!(overlay.ui_theme, config.ui.theme, "ui.theme");
    apply!(overlay.ui_mouse, config.ui.mouse, "ui.mouse");
    apply!(overlay.ui_preview, config.ui.preview, "ui.preview");
    apply!(overlay.editor_mode, config.editor.mode, "editor.mode");
    if let Some(value) = overlay.editor_external {
        config.editor.external = Some(value);
        set_provenance(provenance, "editor.external", &layer.source);
    }
    apply!(overlay.database_path, config.database.path, "database.path");
    apply!(
        overlay.database_busy_timeout_ms,
        config.database.busy_timeout_ms,
        "database.busy_timeout_ms"
    );
    apply!(
        overlay.database_auto_migrate,
        config.database.auto_migrate,
        "database.auto_migrate"
    );
    apply!(
        overlay.database_backup_before_migrate,
        config.database.backup_before_migrate,
        "database.backup_before_migrate"
    );
    apply!(
        overlay.database_backup_keep,
        config.database.backup_keep,
        "database.backup_keep"
    );
    apply!(
        overlay.database_journal_mode,
        config.database.journal_mode,
        "database.journal_mode"
    );
    apply!(overlay.search_field, config.search.field, "search.field");
    apply!(
        overlay.search_matcher,
        config.search.matcher,
        "search.matcher"
    );
    for (name, theme) in overlay.themes {
        config.themes.insert(name.clone(), theme);
        for token in [
            "surface",
            "text",
            "muted",
            "selection",
            "fragment",
            "prompt",
            "tag",
            "success",
            "warning",
            "error",
        ] {
            set_provenance(provenance, &format!("themes.{name}.{token}"), &layer.source);
        }
    }
}

fn validate_effective(config: &Config) -> Result<(), Vec<ConfigDiagnostic>> {
    let mut diagnostics = Vec::new();
    match (&config.editor.mode, &config.editor.external) {
        (EditorMode::External, None) => diagnostics.push(
            ConfigDiagnostic::error(
                "E_CONFIG_EDITOR",
                "external editor mode requires editor.external",
            )
            .with_key("editor.external"),
        ),
        (EditorMode::Builtin, Some(_)) => diagnostics.push(
            ConfigDiagnostic::error(
                "E_CONFIG_EDITOR",
                "builtin editor mode conflicts with editor.external",
            )
            .with_key("editor.external"),
        ),
        _ => {}
    }
    if config.editor.mode == EditorMode::External
        && let Some(argv) = &config.editor.external
    {
        let count = argv.iter().filter(|arg| arg.as_str() == "{file}").count();
        if argv.is_empty() || count != 1 {
            diagnostics.push(
                ConfigDiagnostic::error(
                    "E_CONFIG_EDITOR",
                    "external editor argv must be non-empty and contain `{file}` as a standalone argv element exactly once",
                )
                .with_key("editor.external"),
            );
        }
    }
    if !matches!(config.ui.theme.as_str(), "dark" | "light" | "monochrome")
        && !config.themes.contains_key(&config.ui.theme)
    {
        diagnostics.push(
            ConfigDiagnostic::error(
                "E_CONFIG_THEME",
                format!("selected custom theme `{}` is not defined", config.ui.theme),
            )
            .with_key("ui.theme"),
        );
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn builtin_provenance() -> Provenance {
    Provenance {
        source: ConfigSource::Builtin,
        overridden: Vec::new(),
    }
}

fn config_values(config: &Config) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    values.insert("schema_version".into(), config.schema_version.to_string());
    values.insert(
        "ui.color".into(),
        format!("\"{}\"", config.ui.color.as_str()),
    );
    values.insert(
        "ui.glyphs".into(),
        format!("\"{}\"", config.ui.glyphs.as_str()),
    );
    values.insert("ui.theme".into(), format!("{:?}", config.ui.theme));
    values.insert("ui.mouse".into(), config.ui.mouse.to_string());
    values.insert(
        "ui.preview".into(),
        format!("\"{}\"", config.ui.preview.as_str()),
    );
    values.insert(
        "editor.mode".into(),
        format!("\"{}\"", config.editor.mode.as_str()),
    );
    values.insert(
        "editor.external".into(),
        config
            .editor
            .external
            .as_ref()
            .map(|argv| format!("{argv:?}"))
            .unwrap_or_else(|| "null".into()),
    );
    values.insert(
        "database.path".into(),
        format!("{:?}", config.database.path),
    );
    values.insert(
        "database.busy_timeout_ms".into(),
        config.database.busy_timeout_ms.to_string(),
    );
    values.insert(
        "database.auto_migrate".into(),
        config.database.auto_migrate.to_string(),
    );
    values.insert(
        "database.backup_before_migrate".into(),
        config.database.backup_before_migrate.to_string(),
    );
    values.insert(
        "database.backup_keep".into(),
        config.database.backup_keep.to_string(),
    );
    values.insert(
        "database.journal_mode".into(),
        format!("\"{}\"", config.database.journal_mode.as_str()),
    );
    values.insert(
        "search.field".into(),
        format!("\"{}\"", config.search.field.as_str()),
    );
    values.insert(
        "search.matcher".into(),
        format!("\"{}\"", config.search.matcher.as_str()),
    );
    for (name, theme) in &config.themes {
        for (token, color) in [
            ("surface", &theme.surface),
            ("text", &theme.text),
            ("muted", &theme.muted),
            ("selection", &theme.selection),
            ("fragment", &theme.fragment),
            ("prompt", &theme.prompt),
            ("tag", &theme.tag),
            ("success", &theme.success),
            ("warning", &theme.warning),
            ("error", &theme.error),
        ] {
            values.insert(format!("themes.{name}.{token}"), format!("{color:?}"));
        }
    }
    values
}

fn unknown_key_diagnostics(
    document: &DocumentMut,
    text: &str,
    source: &str,
) -> Vec<ConfigDiagnostic> {
    let top = [
        "schema_version",
        "ui",
        "editor",
        "database",
        "search",
        "themes",
    ];
    let mut diagnostics = Vec::new();
    inspect_table(
        document.as_table(),
        "",
        &top,
        text,
        source,
        &mut diagnostics,
    );
    diagnostics
}

fn inspect_table(
    table: &toml_edit::Table,
    prefix: &str,
    expected: &[&str],
    text: &str,
    source: &str,
    diagnostics: &mut Vec<ConfigDiagnostic>,
) {
    for (key, item) in table.iter() {
        let dotted = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        if !expected.contains(&key) {
            let suggestion = nearest(key, expected);
            let line = item.span().map(|span| {
                text[..span.start.min(text.len())]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1
            });
            let location = line
                .map(|line| format!("{source}:{line}"))
                .unwrap_or_else(|| source.into());
            let mut diagnostic = ConfigDiagnostic::error(
                "E_CONFIG_UNKNOWN_KEY",
                format!("unknown configuration key `{dotted}`"),
            )
            .with_key(dotted)
            .with_source(location);
            if let Some(suggestion) = suggestion {
                diagnostic = diagnostic.with_suggestion(format!("did you mean `{suggestion}`?"));
            }
            diagnostics.push(diagnostic);
            continue;
        }
        match (key, item) {
            ("ui", Item::Table(child)) => inspect_table(
                child,
                "ui",
                &["color", "glyphs", "theme", "mouse", "preview"],
                text,
                source,
                diagnostics,
            ),
            ("editor", Item::Table(child)) => inspect_table(
                child,
                "editor",
                &["mode", "external"],
                text,
                source,
                diagnostics,
            ),
            ("database", Item::Table(child)) => inspect_table(
                child,
                "database",
                &[
                    "path",
                    "busy_timeout_ms",
                    "auto_migrate",
                    "backup_before_migrate",
                    "backup_keep",
                    "journal_mode",
                ],
                text,
                source,
                diagnostics,
            ),
            ("search", Item::Table(child)) => inspect_table(
                child,
                "search",
                &["field", "matcher"],
                text,
                source,
                diagnostics,
            ),
            ("themes", Item::Table(themes)) => {
                for (name, value) in themes.iter() {
                    match value {
                        Item::Table(theme) => inspect_table(
                            theme,
                            &format!("themes.{name}"),
                            theme_keys(),
                            text,
                            source,
                            diagnostics,
                        ),
                        Item::Value(Value::InlineTable(theme)) => inspect_inline(
                            theme,
                            &format!("themes.{name}"),
                            theme_keys(),
                            text,
                            source,
                            diagnostics,
                        ),
                        _ => {}
                    }
                }
            }
            ("ui", Item::Value(Value::InlineTable(child))) => inspect_inline(
                child,
                "ui",
                &["color", "glyphs", "theme", "mouse", "preview"],
                text,
                source,
                diagnostics,
            ),
            ("editor", Item::Value(Value::InlineTable(child))) => inspect_inline(
                child,
                "editor",
                &["mode", "external"],
                text,
                source,
                diagnostics,
            ),
            ("database", Item::Value(Value::InlineTable(child))) => inspect_inline(
                child,
                "database",
                &[
                    "path",
                    "busy_timeout_ms",
                    "auto_migrate",
                    "backup_before_migrate",
                    "backup_keep",
                    "journal_mode",
                ],
                text,
                source,
                diagnostics,
            ),
            ("search", Item::Value(Value::InlineTable(child))) => inspect_inline(
                child,
                "search",
                &["field", "matcher"],
                text,
                source,
                diagnostics,
            ),
            ("themes", Item::Value(Value::InlineTable(themes))) => {
                for (name, value) in themes.iter() {
                    if let Value::InlineTable(theme) = value {
                        inspect_inline(
                            theme,
                            &format!("themes.{name}"),
                            theme_keys(),
                            text,
                            source,
                            diagnostics,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn inspect_inline(
    table: &InlineTable,
    prefix: &str,
    expected: &[&str],
    text: &str,
    source: &str,
    diagnostics: &mut Vec<ConfigDiagnostic>,
) {
    for (key, value) in table.iter() {
        if expected.contains(&key) {
            continue;
        }
        let dotted = format!("{prefix}.{key}");
        let line = value.span().map(|span| {
            text[..span.start.min(text.len())]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1
        });
        let location = line
            .map(|line| format!("{source}:{line}"))
            .unwrap_or_else(|| source.into());
        let mut diagnostic = ConfigDiagnostic::error(
            "E_CONFIG_UNKNOWN_KEY",
            format!("unknown configuration key `{dotted}`"),
        )
        .with_key(dotted)
        .with_source(location);
        if let Some(suggestion) = nearest(key, expected) {
            diagnostic = diagnostic.with_suggestion(format!("did you mean `{suggestion}`?"));
        }
        diagnostics.push(diagnostic);
    }
}

fn theme_keys() -> &'static [&'static str] {
    &[
        "surface",
        "text",
        "muted",
        "selection",
        "fragment",
        "prompt",
        "tag",
        "success",
        "warning",
        "error",
    ]
}

fn nearest<'a>(actual: &str, expected: &'a [&str]) -> Option<&'a str> {
    expected
        .iter()
        .map(|candidate| (*candidate, edit_distance(actual, candidate)))
        .min_by_key(|(_, distance)| *distance)
        .and_then(|(candidate, distance)| (distance <= 3).then_some(candidate))
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    for (row, left_char) in left.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, right_char) in right_chars.iter().enumerate() {
            current.push(
                (current[column] + 1)
                    .min(previous[column + 1] + 1)
                    .min(previous[column] + usize::from(left_char != *right_char)),
            );
        }
        previous = current;
    }
    previous[right_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn non_unicode_os_string() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xff])
    }

    #[cfg(windows)]
    fn non_unicode_os_string() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xd800])
    }

    fn paths() -> ConfigPaths {
        ConfigPaths {
            user_config: "user.toml".into(),
            database: "default.db".into(),
            state_dir: "state".into(),
            cache_dir: "cache".into(),
            backup_dir: "backup".into(),
        }
    }

    #[test]
    fn unknown_key_has_edit_distance_suggestion() {
        let parsed =
            ParsedConfig::parse("schema_version=1\n[ui]\ncolr='auto'\n", "test.toml").unwrap();
        assert_eq!(
            parsed.diagnostics[0].suggestion.as_deref(),
            Some("did you mean `color`?")
        );
    }

    #[test]
    fn later_layers_win_and_retain_provenance() {
        let loader = ConfigLoader::new(paths());
        let user = ConfigOverlay {
            ui_color: Some(ColorMode::Ansi16),
            ..ConfigOverlay::default()
        };
        let cli = ConfigOverlay {
            ui_color: Some(ColorMode::None),
            ..ConfigOverlay::default()
        };
        let loaded = loader
            .merge(vec![
                ConfigLayer {
                    source: ConfigSource::UserFile("user.toml".into()),
                    overlay: user,
                },
                ConfigLayer {
                    source: ConfigSource::Cli("--color".into()),
                    overlay: cli,
                },
            ])
            .unwrap();
        assert_eq!(loaded.config.ui.color, ColorMode::None);
        assert_eq!(
            loaded.provenance["ui.color"].source,
            ConfigSource::Cli("--color".into())
        );
        assert!(
            loaded.provenance["ui.color"]
                .overridden
                .iter()
                .any(|source| matches!(source, ConfigSource::UserFile(_)))
        );
    }

    #[test]
    fn parser_preserves_comments() {
        let parsed = ParsedConfig::parse("# hello\nschema_version = 1\n", "memory").unwrap();
        assert!(parsed.document.to_string().starts_with("# hello"));
    }

    #[test]
    fn complete_precedence_chain_is_field_wise() {
        let directory = tempfile::tempdir().unwrap();
        let user_path = directory.path().join("user.toml");
        let explicit_path = directory.path().join("explicit.toml");
        fs::write(&user_path, "schema_version=1\n[ui]\ncolor='ansi16'\n").unwrap();
        fs::write(&explicit_path, "schema_version=1\n[ui]\ncolor='ansi256'\n").unwrap();
        let mut environment = Environment::default();
        environment
            .values
            .insert("PROMPTR_COLOR".into(), "truecolor".into());
        let cli = ConfigOverlay {
            ui_color: Some(ColorMode::None),
            ..ConfigOverlay::default()
        };
        let loaded = ConfigLoader::new(paths())
            .load(LoadRequest {
                user_config: Some(user_path),
                explicit_config: Some(explicit_path),
                environment,
                cli,
                ..LoadRequest::default()
            })
            .unwrap();
        assert_eq!(loaded.config.ui.color, ColorMode::None);
        let chain = &loaded.provenance["ui.color"];
        assert!(matches!(chain.source, ConfigSource::Cli(_)));
        assert!(matches!(chain.overridden[0], ConfigSource::Builtin));
        assert!(matches!(chain.overridden[1], ConfigSource::UserFile(_)));
        assert!(matches!(chain.overridden[2], ConfigSource::ExplicitFile(_)));
        assert!(matches!(chain.overridden[3], ConfigSource::Environment(_)));
    }

    #[test]
    fn newer_schema_returns_compatibility_diagnostic() {
        let diagnostic = ParsedConfig::parse(
            "schema_version=99\n[ui]\ncolor='a-future-enum'\n",
            "future.toml",
        )
        .unwrap_err()
        .remove(0);
        assert_eq!(diagnostic.code, "E_CONFIG_SCHEMA_NEW");
        assert!(diagnostic.message.contains("found=99"));
        assert!(diagnostic.message.contains("max_supported=1"));
        assert!(diagnostic.message.contains("path=future.toml"));
        assert!(diagnostic.message.contains("app_version="));
    }

    #[test]
    fn inline_tables_receive_recursive_unknown_key_diagnostics() {
        let theme = "{ surface='#000000', text='#ffffff', muted='#777777', selection='#111111', fragment='blue', prompt='magenta', tag='cyan', success='green', warning='yellow', error='red', warrning='red' }";
        let source = format!(
            "schema_version=1\nui={{ color='auto', colr='auto' }}\nthemes={{ moe={theme} }}\n"
        );
        let parsed = ParsedConfig::parse(&source, "inline.toml").unwrap();
        let keys = parsed
            .diagnostics
            .iter()
            .filter_map(|value| value.key.as_deref())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"ui.colr"));
        assert!(keys.contains(&"themes.moe.warrning"));
    }

    #[test]
    fn environment_preserves_non_unicode_paths_and_diagnoses_enum_values() {
        let path = non_unicode_os_string();
        let mut environment = Environment::default();
        environment
            .values
            .insert("PROMPTR_DATABASE".into(), path.clone());
        environment
            .values
            .insert("PROMPTR_COLOR".into(), non_unicode_os_string());
        let mut diagnostics = Vec::new();
        let layers = environment_layers(&environment, &mut diagnostics);
        assert_eq!(layers[0].overlay.database_path, Some(PathBuf::from(path)));
        assert!(
            diagnostics
                .iter()
                .any(|value| value.code == "E_CONFIG_ENV_UNICODE")
        );
    }

    #[test]
    fn old_schema_is_migrated_in_memory_without_rewrite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("old.toml");
        let original = b"schema_version=0\n[ui]\nmouse=false\n";
        fs::write(&path, original).unwrap();
        let loaded = ConfigLoader::new(paths())
            .load(LoadRequest {
                explicit_config: Some(path.clone()),
                ..LoadRequest::default()
            })
            .unwrap();
        assert!(!loaded.config.ui.mouse);
        assert_eq!(
            loaded.config.schema_version,
            super::super::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(fs::read(path).unwrap(), original);
        assert!(
            loaded
                .diagnostics
                .iter()
                .any(|value| value.code == "W_CONFIG_MIGRATION_AVAILABLE")
        );
    }
}
