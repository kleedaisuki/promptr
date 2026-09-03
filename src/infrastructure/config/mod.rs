//! 配置基础设施 / Configuration infrastructure.
//!
//! 该模块把无损 TOML 文档、版本化原始结构与运行时强类型配置分离。

mod diagnostic;
mod loader;
mod migration;
mod paths;
mod schema;

pub use diagnostic::{ConfigDiagnostic, DiagnosticSeverity};
pub use loader::{
    ConfigLayer, ConfigLoader, ConfigSource, EffectiveConfig, EffectiveEntry, Environment,
    ExplainValue, LoadRequest, LoadedConfig, ParsedConfig, Provenance,
};
pub use migration::{MigrationOutcome, check_file, migrate_file};
pub use paths::ConfigPaths;
pub use schema::{
    CURRENT_SCHEMA_VERSION, ColorMode, Config, ConfigOverlay, DatabaseConfig, EditorConfig,
    EditorMode, GlyphMode, PreviewMode, RawConfig, SearchConfig, SearchField, SearchMatcher, Theme,
    UiConfig, init_example,
};
