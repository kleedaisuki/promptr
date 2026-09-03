//! 命令行宿主与组合根。 / Command-line host and composition root.

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use clap::{Args, Parser, Subcommand, ValueEnum, error::ErrorKind};
use serde_json::json;

use crate::{
    Diagnostic, DiagnosticCategory, InvocationPolicy, Promptr, PromptrOptions,
    infrastructure::{
        config::{
            ConfigDiagnostic, ConfigLoader, ConfigOverlay, ConfigPaths, DatabaseConfig,
            Environment, LoadRequest, LoadedConfig, MigrationOutcome, check_file, init_example,
            migrate_file,
        },
        sqlite::{SCHEMA_VERSION, SqliteDatabase, SqliteOptions},
    },
};

use super::{
    presenter::{OutputFormat, write_data, write_diagnostic, write_values},
    repl, tui,
};

/// @brief Promptr 命令行参数。 / Promptr command-line arguments.
#[derive(Debug, Parser)]
#[command(
    version,
    about = "Manage, compose, search, and render prompt catalogs",
    arg_required_else_help = false
)]
pub struct Cli {
    /// @brief 覆盖数据库路径。 / Override the database path.
    #[arg(
        long,
        global = true,
        env = "PROMPTR_DATABASE",
        help = "Override the database path"
    )]
    pub database: Option<PathBuf>,
    /// @brief 叠加显式配置文件。 / Overlay an explicit configuration file.
    #[arg(
        long,
        global = true,
        env = "PROMPTR_CONFIG",
        conflicts_with = "no_config",
        help = "Overlay an explicit configuration file"
    )]
    pub config: Option<PathBuf>,
    /// @brief 禁用所有配置文件层。 / Disable every configuration-file layer.
    #[arg(long, global = true, help = "Disable every configuration-file layer")]
    pub no_config: bool,
    /// @brief 选择输出契约。 / Select the output contract.
    #[arg(
        long,
        global = true,
        value_enum,
        default_value_t,
        hide_possible_values = true,
        help = "Select the output contract"
    )]
    pub format: OutputFormat,
    /// @brief 覆盖终端颜色策略。 / Override the terminal color policy.
    #[arg(long, global = true, value_enum, help = "Override terminal colors")]
    pub color: Option<CliColor>,
    /// @brief 覆盖终端字形策略。 / Override the terminal glyph policy.
    #[arg(long, global = true, value_enum, help = "Override terminal glyphs")]
    pub glyphs: Option<CliGlyphs>,
    /// @brief 覆盖 TUI 主题。 / Override the TUI theme.
    #[arg(long, global = true, help = "Override the TUI theme")]
    pub theme: Option<String>,
    /// @brief 覆盖默认预览投影。 / Override the default preview projection.
    #[arg(long, global = true, value_enum, help = "Override the default preview")]
    pub preview: Option<CliPreview>,
    /// @brief 显式启用或禁用鼠标。 / Explicitly enable or disable mouse input.
    #[arg(
        long,
        global = true,
        action = clap::ArgAction::Set,
        value_parser = clap::value_parser!(bool),
        value_name = "true|false",
        help = "Explicitly enable or disable mouse input"
    )]
    pub mouse: Option<bool>,
    /// @brief 可选操作；缺省进入交互宿主。 / Optional operation; absent enters an interactive host.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// @brief CLI 专用颜色字面量。 / CLI-local color literals.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CliColor {
    /// @brief 自动探测。 / Detect automatically.
    #[value(help = "Detect color capability automatically")]
    Auto,
    /// @brief 真彩色。 / True color.
    #[value(help = "Use 24-bit true color")]
    Truecolor,
    /// @brief 256 色 ANSI。 / 256-color ANSI.
    #[value(help = "Use the ANSI 256-color palette")]
    Ansi256,
    /// @brief 16 色 ANSI。 / 16-color ANSI.
    #[value(help = "Use the ANSI 16-color palette")]
    Ansi16,
    /// @brief 禁用颜色。 / Disable color.
    #[value(help = "Disable color")]
    None,
}

/// @brief CLI 专用字形字面量。 / CLI-local glyph literals.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CliGlyphs {
    /// @brief 自动探测。 / Detect automatically.
    #[value(help = "Detect glyph capability automatically")]
    Auto,
    /// @brief 使用 Unicode 字形。 / Use Unicode glyphs.
    #[value(help = "Use Unicode glyphs")]
    Unicode,
    /// @brief 使用 ASCII 字形。 / Use ASCII glyphs.
    #[value(help = "Use ASCII glyphs")]
    Ascii,
}

/// @brief CLI 专用预览字面量。 / CLI-local preview literals.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CliPreview {
    /// @brief 树投影。 / Tree projection.
    #[value(help = "Start with the tree preview")]
    Tree,
    /// @brief XML 投影。 / XML projection.
    #[value(help = "Start with the XML preview")]
    Xml,
    /// @brief 正文投影。 / Content projection.
    #[value(help = "Start with the content preview")]
    Content,
    /// @brief 元数据投影。 / Metadata projection.
    #[value(help = "Start with the metadata preview")]
    Metadata,
}

impl From<CliColor> for crate::infrastructure::config::ColorMode {
    /// @brief 将 CLI 颜色映射为配置领域值。 / Map a CLI color into the configuration value.
    /// @param value CLI 颜色。 / CLI color.
    /// @return 配置颜色。 / Configuration color.
    fn from(value: CliColor) -> Self {
        match value {
            CliColor::Auto => Self::Auto,
            CliColor::Truecolor => Self::Truecolor,
            CliColor::Ansi256 => Self::Ansi256,
            CliColor::Ansi16 => Self::Ansi16,
            CliColor::None => Self::None,
        }
    }
}

impl From<CliGlyphs> for crate::infrastructure::config::GlyphMode {
    /// @brief 将 CLI 字形映射为配置领域值。 / Map CLI glyphs into the configuration value.
    /// @param value CLI 字形。 / CLI glyphs.
    /// @return 配置字形。 / Configuration glyphs.
    fn from(value: CliGlyphs) -> Self {
        match value {
            CliGlyphs::Auto => Self::Auto,
            CliGlyphs::Unicode => Self::Unicode,
            CliGlyphs::Ascii => Self::Ascii,
        }
    }
}

impl From<CliPreview> for crate::infrastructure::config::PreviewMode {
    /// @brief 将 CLI 预览映射为配置领域值。 / Map a CLI preview into the configuration value.
    /// @param value CLI 预览。 / CLI preview.
    /// @return 配置预览。 / Configuration preview.
    fn from(value: CliPreview) -> Self {
        match value {
            CliPreview::Tree => Self::Tree,
            CliPreview::Xml => Self::Xml,
            CliPreview::Content => Self::Content,
            CliPreview::Metadata => Self::Metadata,
        }
    }
}

/// @brief 顶层命令。 / Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// @brief 原子执行脚本文件或标准输入。 / Atomically run a script file or standard input.
    #[command(about = "Atomically run a script file or standard input")]
    Run {
        #[arg(help = "Script path, or '-' for standard input")]
        path: String,
    },
    /// @brief 执行一段命令行源码。 / Evaluate command-line source.
    #[command(about = "Evaluate command-line DSL source")]
    Eval {
        #[arg(help = "DSL source to evaluate")]
        source: String,
    },
    /// @brief 仅检查脚本语法和语义。 / Check script syntax and semantics only.
    #[command(about = "Check script syntax and semantics without executing it")]
    Check {
        #[arg(help = "Script path to check")]
        path: PathBuf,
    },
    /// @brief 管理配置。 / Manage configuration.
    #[command(about = "Manage configuration")]
    Config(ConfigArgs),
    /// @brief 管理 SQLite 数据库。 / Manage the SQLite database.
    #[command(about = "Manage the SQLite database")]
    Db(DbArgs),
    /// @brief 汇总环境、配置与数据库状态。 / Summarize environment, configuration, and database state.
    #[command(about = "Summarize environment, configuration, and database state")]
    Doctor,
}

/// @brief 配置命令参数。 / Configuration command arguments.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// @brief 配置操作。 / Configuration operation.
    #[command(subcommand)]
    pub command: ConfigCommand,
}

/// @brief 配置管理操作。 / Configuration management operations.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// @brief 显示平台用户配置路径。 / Show the platform user-config path.
    #[command(about = "Show the platform user-config path")]
    Path,
    /// @brief 创建带注释的示例配置。 / Create a commented example configuration.
    #[command(about = "Create a commented example configuration")]
    Init,
    /// @brief 检查配置文件且不改写。 / Check a config file without rewriting it.
    #[command(about = "Check a configuration file without rewriting it")]
    Check {
        #[arg(help = "Configuration path; defaults to the selected user config")]
        path: Option<PathBuf>,
    },
    /// @brief 显示全部生效值和来源。 / Show all effective values and provenance.
    #[command(about = "Show effective configuration and provenance")]
    Show {
        /// @brief 明确请求生效视图；目前这是唯一视图。 / Explicitly request the effective view; currently the only view.
        #[arg(long, help = "Explicitly request the effective view")]
        effective: bool,
    },
    /// @brief 解释一个点分配置键。 / Explain one dotted configuration key.
    #[command(about = "Explain one dotted configuration key")]
    Explain {
        #[arg(help = "Dotted configuration key")]
        key: String,
    },
    /// @brief 显式迁移配置文件。 / Explicitly migrate a configuration file.
    #[command(about = "Explicitly migrate the selected configuration file")]
    Migrate {
        /// @brief 只报告是否需要迁移。 / Only report whether migration is required.
        #[arg(long, help = "Report whether migration is required without rewriting")]
        check: bool,
    },
}

/// @brief 数据库命令参数。 / Database command arguments.
#[derive(Debug, Args)]
pub struct DbArgs {
    /// @brief 数据库操作。 / Database operation.
    #[command(subcommand)]
    pub command: DbCommand,
}

/// @brief 数据库维护操作。 / Database maintenance operations.
#[derive(Debug, Subcommand)]
pub enum DbCommand {
    /// @brief 显示数据库身份与版本。 / Show database identity and version.
    #[command(about = "Show database identity and version")]
    Status,
    /// @brief 检查或执行数据库迁移。 / Check or apply database migrations.
    #[command(about = "Check or apply database migrations")]
    Migrate {
        /// @brief 仅检查。 / Check only.
        #[arg(long, help = "Check migration status without applying changes")]
        check: bool,
    },
    /// @brief 创建 SQLite 一致备份。 / Create a SQLite-consistent backup.
    #[command(about = "Create a SQLite-consistent backup")]
    Backup {
        #[arg(help = "Backup destination path")]
        path: Option<PathBuf>,
    },
    /// @brief 检查物理与领域一致性。 / Check physical and domain consistency.
    #[command(about = "Check physical and domain consistency")]
    Check {
        /// @brief 运行完整 integrity_check。 / Run the full integrity_check.
        #[arg(long, help = "Run the full SQLite integrity check")]
        full: bool,
    },
    /// @brief 从规范表重建搜索索引。 / Rebuild search indexes from canonical tables.
    #[command(about = "Rebuild search indexes from canonical tables")]
    RebuildIndex,
}

/// @brief 从真实进程流运行 CLI。 / Run the CLI with real process streams.
/// @return 适合作为 `main` 返回值的退出码。 / Exit code suitable for `main`.
pub fn main_entry() -> ExitCode {
    let arguments = env::args_os().collect::<Vec<_>>();
    let early_format = early_output_format(&arguments);
    let cli = match Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error) => {
            let mut stdout = io::stdout().lock();
            let mut stderr = io::stderr().lock();
            return ExitCode::from(write_clap_error(
                error,
                early_format,
                &mut stdout,
                &mut stderr,
            ));
        }
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    if should_use_tui(&cli, stdin.is_terminal(), stdout.is_terminal()) {
        return ExitCode::from(run_tui(cli));
    }
    let mut input = stdin.lock();
    let mut stdout = stdout.lock();
    let mut stderr = io::stderr().lock();
    ExitCode::from(run(cli, &mut input, &mut stdout, &mut stderr))
}

/// @brief 在完整解析前识别机器输出请求。 / Recognize a machine-output request before full parsing.
/// @param arguments 原始进程参数，包括程序名。 / Raw process arguments including the program name.
/// @return 仅识别精确 `--format=json` 或 `--format json`；其余回退 human。 / Recognizes only exact `--format=json` or `--format json`; all other input falls back to human.
fn early_output_format(arguments: &[OsString]) -> OutputFormat {
    let mut format = OutputFormat::Human;
    let mut index = 1;
    while index < arguments.len() {
        if arguments[index] == OsStr::new("--format=json") {
            format = OutputFormat::Json;
        } else if arguments[index] == OsStr::new("--format")
            && arguments
                .get(index + 1)
                .is_some_and(|value| value == "json")
        {
            format = OutputFormat::Json;
            index += 1;
        }
        index += 1;
    }
    format
}

/// @brief 发布 Clap 的早期终止或参数错误。 / Publish an early Clap exit or argument error.
/// @param error Clap 产生的结构化错误。 / Structured error produced by Clap.
/// @param format 完整解析前识别的输出格式。 / Output format recognized before full parsing.
/// @param stdout 标准输出。 / Standard output.
/// @param stderr 标准错误。 / Standard error.
/// @return 稳定退出码。 / Stable exit code.
fn write_clap_error(
    error: clap::Error,
    format: OutputFormat,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        let code = u8::try_from(error.exit_code()).unwrap_or(0);
        let _ = stdout.write_all(error.render().to_string().as_bytes());
        return code;
    }
    if format == OutputFormat::Json {
        let diagnostic =
            Diagnostic::error("E_CLI_PARSE", DiagnosticCategory::Syntax, error.to_string());
        let _ = write_diagnostic(stdout, stderr, OutputFormat::Json, &diagnostic);
        2
    } else {
        let code = u8::try_from(error.exit_code()).unwrap_or(2);
        let _ = stderr.write_all(error.render().to_string().as_bytes());
        code
    }
}

/// @brief 判断真实终端调用是否应进入 TUI。 / Decide whether a real-terminal invocation should enter the TUI.
/// @param cli 已解析命令行。 / Parsed command line.
/// @param stdin_terminal 标准输入是否为终端。 / Whether standard input is a terminal.
/// @param stdout_terminal 标准输出是否为终端。 / Whether standard output is a terminal.
/// @return 仅交互 human 模式且两端均为终端时为真。 / True only for interactive human mode with both terminal endpoints.
fn should_use_tui(cli: &Cli, stdin_terminal: bool, stdout_terminal: bool) -> bool {
    cli.command.is_none() && cli.format == OutputFormat::Human && stdin_terminal && stdout_terminal
}

/// @brief 装配应用并运行真实终端 TUI。 / Assemble the application and run the real-terminal TUI.
/// @param cli 已确认适用于 TUI 的命令行。 / Command line already confirmed suitable for the TUI.
/// @return 稳定退出码。 / Stable exit code.
fn run_tui(cli: Cli) -> u8 {
    let result = (|| {
        let paths = ConfigPaths::discover().map_err(config_error)?;
        let loaded = load_config(&cli, &paths)?;
        let database_path = cli
            .database
            .clone()
            .unwrap_or_else(|| loaded.config.database.path.clone());
        ensure_parent(&database_path).map_err(|error| io_error(&database_path, error))?;
        let mut app = Promptr::open(PromptrOptions {
            database_path: Some(database_path),
            config: loaded.config,
        })?;
        tui::run(&mut app)
    })();
    match result {
        Ok(()) => 0,
        Err(diagnostic) => {
            let mut stdout = io::stdout().lock();
            let mut stderr = io::stderr().lock();
            fail(&mut stdout, &mut stderr, OutputFormat::Human, diagnostic)
        }
    }
}

/// @brief 以可注入流执行已解析参数。 / Execute parsed arguments with injectable streams.
/// @param cli 已解析命令行。 / Parsed command line.
/// @param input 标准输入。 / Standard input.
/// @param stdout 标准输出。 / Standard output.
/// @param stderr 标准错误。 / Standard error.
/// @return 稳定退出码。 / Stable exit code.
pub fn run(
    cli: Cli,
    input: &mut dyn BufRead,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    if cli.format == OutputFormat::Raw
        && !matches!(
            cli.command,
            Some(Command::Eval { .. }) | Some(Command::Run { .. })
        )
    {
        return fail(
            stdout,
            stderr,
            cli.format,
            Diagnostic::error(
                "E_CLI_RAW",
                DiagnosticCategory::Capability,
                "--format raw is only valid for run/eval results containing only OUTPUT XML",
            ),
        );
    }
    match execute(cli, input, stdout, stderr) {
        Ok(ExecuteOutcome::Complete) => 0,
        Ok(ExecuteOutcome::Repl(outcome)) => outcome.exit_code(),
        Err((format, diagnostic)) => fail(stdout, stderr, format, diagnostic),
    }
}

/// @brief CLI 分派的成功结果。 / Successful CLI dispatch outcome.
enum ExecuteOutcome {
    /// @brief 非交互命令成功完成。 / A non-interactive command completed successfully.
    Complete,
    /// @brief REPL 已自行发布任何致命诊断。 / The REPL has published any fatal diagnostic itself.
    Repl(repl::ReplOutcome),
}

fn execute(
    mut cli: Cli,
    input: &mut dyn BufRead,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<ExecuteOutcome, (OutputFormat, Diagnostic)> {
    let format = cli.format;
    let paths = ConfigPaths::discover().map_err(|error| (format, config_error(error)))?;
    let command = cli.command.take();
    match command {
        Some(Command::Config(args)) => {
            run_config(args.command, &cli, &paths, stdout).map(|()| ExecuteOutcome::Complete)
        }
        command => {
            let loaded = load_config(&cli, &paths).map_err(|error| (format, error))?;
            let database_path = cli
                .database
                .clone()
                .unwrap_or_else(|| loaded.config.database.path.clone());
            ensure_parent(&database_path)
                .map_err(|error| (format, io_error(&database_path, error)))?;
            match command {
                Some(Command::Db(args)) => run_db(
                    args.command,
                    &paths,
                    &database_path,
                    &loaded.config.database,
                    format,
                    stdout,
                )
                .map(|()| ExecuteOutcome::Complete),
                Some(Command::Doctor) => run_doctor(&loaded, &database_path, format, stdout)
                    .map(|()| ExecuteOutcome::Complete),
                Some(Command::Run { path }) => {
                    let source = if path == "-" {
                        let mut source = String::new();
                        input
                            .read_to_string(&mut source)
                            .map_err(|error| (format, io_error(Path::new("<stdin>"), error)))?;
                        source
                    } else {
                        fs::read_to_string(&path)
                            .map_err(|error| (format, io_error(Path::new(&path), error)))?
                    };
                    eval_source(loaded, database_path, &source, format, stdout)
                        .map(|()| ExecuteOutcome::Complete)
                }
                Some(Command::Eval { source }) => {
                    eval_source(loaded, database_path, &source, format, stdout)
                        .map(|()| ExecuteOutcome::Complete)
                }
                Some(Command::Check { path }) => {
                    let source = fs::read_to_string(&path)
                        .map_err(|error| (format, io_error(&path, error)))?;
                    let app = Promptr::open(PromptrOptions {
                        database_path: Some(database_path),
                        config: loaded.config,
                    })
                    .map_err(|error| (format, error))?;
                    app.check(&source, InvocationPolicy::interactive())
                        .map_err(|error| (format, error))?;
                    emit(stdout, format, json!({"checked": path}), "check succeeded")
                        .map(|()| ExecuteOutcome::Complete)
                }
                None => {
                    if format != OutputFormat::Human {
                        return Err((
                            format,
                            Diagnostic::error(
                                "E_CLI_INTERACTIVE_FORMAT",
                                DiagnosticCategory::Capability,
                                "interactive mode requires human format",
                            ),
                        ));
                    }
                    let mut app = Promptr::open(PromptrOptions {
                        database_path: Some(database_path),
                        config: loaded.config,
                    })
                    .map_err(|error| (format, error))?;
                    Ok(ExecuteOutcome::Repl(repl::run(
                        &mut app, input, stdout, stderr,
                    )))
                }
                Some(Command::Config(_)) => unreachable!(),
            }
        }
    }
}

fn load_config(cli: &Cli, paths: &ConfigPaths) -> Result<LoadedConfig, Diagnostic> {
    let overlay = ConfigOverlay {
        database_path: cli.database.clone(),
        ui_color: cli.color.map(Into::into),
        ui_glyphs: cli.glyphs.map(Into::into),
        ui_theme: cli.theme.clone(),
        ui_mouse: cli.mouse,
        ui_preview: cli.preview.map(Into::into),
        ..ConfigOverlay::default()
    };
    ConfigLoader::new(paths.clone())
        .load(LoadRequest {
            no_config: cli.no_config,
            explicit_config: cli.config.clone(),
            environment: Environment::current(),
            cli: overlay,
            ..LoadRequest::default()
        })
        .map_err(config_errors)
}

fn eval_source(
    loaded: LoadedConfig,
    database_path: PathBuf,
    source: &str,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<(), (OutputFormat, Diagnostic)> {
    let mut app = Promptr::open(PromptrOptions {
        database_path: Some(database_path),
        config: loaded.config,
    })
    .map_err(|error| (format, error))?;
    if format == OutputFormat::Raw {
        let checked = app
            .check(source, InvocationPolicy::script())
            .map_err(|error| (format, error))?;
        if checked
            .ops
            .iter()
            .any(|operation| !matches!(operation, crate::application::Op::RenderXml { .. }))
        {
            return Err((
                format,
                Diagnostic::error(
                    "E_OUTPUT_FORMAT",
                    DiagnosticCategory::Capability,
                    "raw format requires every operation to be OUTPUT",
                ),
            ));
        }
    }
    let values = app
        .eval(source, InvocationPolicy::script())
        .map_err(|error| (format, error))?;
    write_values(stdout, format, &values).map_err(|error| {
        (
            format,
            Diagnostic::error(
                "E_OUTPUT_FORMAT",
                DiagnosticCategory::Capability,
                error.to_string(),
            ),
        )
    })
}

fn run_config(
    command: ConfigCommand,
    cli: &Cli,
    paths: &ConfigPaths,
    stdout: &mut dyn Write,
) -> Result<(), (OutputFormat, Diagnostic)> {
    let format = cli.format;
    let selected = cli.config.as_deref().unwrap_or(&paths.user_config);
    match command {
        ConfigCommand::Path => emit(
            stdout,
            format,
            json!({"path": paths.user_config}),
            &paths.user_config.display().to_string(),
        ),
        ConfigCommand::Init => {
            if selected.exists() {
                return Err((
                    format,
                    Diagnostic::error(
                        "E_CONFIG_EXISTS",
                        DiagnosticCategory::Configuration,
                        format!("configuration already exists at `{}`", selected.display()),
                    ),
                ));
            }
            ensure_parent(selected).map_err(|error| (format, io_error(selected, error)))?;
            fs::write(selected, init_example())
                .map_err(|error| (format, io_error(selected, error)))?;
            emit(
                stdout,
                format,
                json!({"initialized": selected}),
                &format!("initialized {}", selected.display()),
            )
        }
        ConfigCommand::Check { path } => {
            let path = path.as_deref().unwrap_or(selected);
            let outcome = check_file(path).map_err(|errors| (format, config_errors(errors)))?;
            emit_migration(stdout, format, path, &outcome)
        }
        ConfigCommand::Migrate { check } => {
            let outcome = if check {
                check_file(selected)
            } else {
                migrate_file(selected)
            }
            .map_err(|errors| (format, config_errors(errors)))?;
            emit_migration(stdout, format, selected, &outcome)
        }
        ConfigCommand::Show { effective: _ } => {
            let loaded = load_config(cli, paths).map_err(|error| (format, error))?;
            write_data(stdout, format, &loaded.effective())
                .map_err(|error| (format, output_error(error)))
        }
        ConfigCommand::Explain { key } => {
            let loaded = load_config(cli, paths).map_err(|error| (format, error))?;
            let value = loaded.explain(&key).ok_or_else(|| {
                (
                    format,
                    Diagnostic::error(
                        "E_CONFIG_KEY",
                        DiagnosticCategory::Configuration,
                        format!("unknown configuration key `{key}`"),
                    ),
                )
            })?;
            write_data(stdout, format, &value).map_err(|error| (format, output_error(error)))
        }
    }
}

fn run_db(
    command: DbCommand,
    paths: &ConfigPaths,
    database_path: &Path,
    database_config: &DatabaseConfig,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<(), (OutputFormat, Diagnostic)> {
    let mut database =
        SqliteDatabase::open_existing_with_options(database_path, sqlite_options(database_config))
            .map_err(|error| (format, error))?;
    match command {
        DbCommand::Status => {
            let value = database.status().map_err(|error| (format, error))?;
            emit(
                stdout,
                format,
                json!({"path": value.path, "application_id": value.application_id, "user_version": value.user_version, "ledger_version": value.ledger_version, "compatible": value.compatible}),
                &format!(
                    "{}: schema {}, compatible={}",
                    database_path.display(),
                    value.user_version,
                    value.compatible
                ),
            )
        }
        DbCommand::Migrate { check } => {
            let status = database.status().map_err(|error| (format, error))?;
            if !status.compatible || status.user_version != SCHEMA_VERSION {
                return Err((
                    format,
                    Diagnostic::error(
                        "E_DB_MIGRATION",
                        DiagnosticCategory::Compatibility,
                        format!(
                            "no supported migration from schema {} to {SCHEMA_VERSION}",
                            status.user_version
                        ),
                    ),
                ));
            }
            emit(
                stdout,
                format,
                json!({"current": true, "check": check, "schema_version": SCHEMA_VERSION}),
                "database schema is current",
            )
        }
        DbCommand::Backup { path } => {
            let destination = path.unwrap_or_else(|| default_backup(paths, database_path));
            ensure_parent(&destination).map_err(|error| (format, io_error(&destination, error)))?;
            database
                .backup(&destination)
                .map_err(|error| (format, error))?;
            emit(
                stdout,
                format,
                json!({"backup": destination}),
                &format!("backup written to {}", destination.display()),
            )
        }
        DbCommand::Check { full } => {
            let report = database.check(full).map_err(|error| (format, error))?;
            emit(
                stdout,
                format,
                json!({"integrity": report.integrity, "foreign_key_violations": report.foreign_key_violations, "domain_valid": report.domain_valid}),
                &format!(
                    "integrity: {}; foreign-key violations: {}; domain valid: {}",
                    report.integrity.join(", "),
                    report.foreign_key_violations,
                    report.domain_valid
                ),
            )
        }
        DbCommand::RebuildIndex => {
            database.rebuild_index().map_err(|error| (format, error))?;
            emit(
                stdout,
                format,
                json!({"rebuilt": true}),
                "search index rebuilt",
            )
        }
    }
}

fn run_doctor(
    loaded: &LoadedConfig,
    path: &Path,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<(), (OutputFormat, Diagnostic)> {
    let database =
        SqliteDatabase::open_existing_with_options(path, sqlite_options(&loaded.config.database))
            .map_err(|error| (format, error))?;
    let status = database.status().map_err(|error| (format, error))?;
    let check = database.check(false).map_err(|error| (format, error))?;
    emit(
        stdout,
        format,
        json!({"application_version": env!("CARGO_PKG_VERSION"), "config_schema": loaded.config.schema_version, "database": {"path": path, "schema": status.user_version, "compatible": status.compatible}, "check": {"integrity": check.integrity, "foreign_key_violations": check.foreign_key_violations, "domain_valid": check.domain_valid}, "config_warnings": loaded.diagnostics.iter().map(|item| item.to_string()).collect::<Vec<_>>() }),
        &format!(
            "promptr {}; config schema {}; database schema {}; integrity {}",
            env!("CARGO_PKG_VERSION"),
            loaded.config.schema_version,
            status.user_version,
            check.integrity.join(", ")
        ),
    )
}

fn emit(
    stdout: &mut dyn Write,
    format: OutputFormat,
    value: serde_json::Value,
    human: &str,
) -> Result<(), (OutputFormat, Diagnostic)> {
    let result = match format {
        OutputFormat::Json => write_data(stdout, format, &value),
        OutputFormat::Human => writeln!(stdout, "{human}"),
        OutputFormat::Raw => unreachable!("raw management commands rejected before dispatch"),
    };
    result.map_err(|error| (format, output_error(error)))
}

fn emit_migration(
    stdout: &mut dyn Write,
    format: OutputFormat,
    path: &Path,
    outcome: &MigrationOutcome,
) -> Result<(), (OutputFormat, Diagnostic)> {
    let (value, human) = match outcome {
        MigrationOutcome::AlreadyCurrent => (
            json!({"path": path, "status": "current"}),
            format!("{} is current", path.display()),
        ),
        MigrationOutcome::WouldMigrate { from, to } => (
            json!({"path": path, "status": "would_migrate", "from": from, "to": to}),
            format!("{} would migrate from {from} to {to}", path.display()),
        ),
        MigrationOutcome::Migrated { from, to, backup } => (
            json!({"path": path, "status": "migrated", "from": from, "to": to, "backup": backup}),
            format!(
                "{} migrated from {from} to {to}; backup {}",
                path.display(),
                backup.display()
            ),
        ),
    };
    emit(stdout, format, value, &human)
}

fn fail(
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
    diagnostic: Diagnostic,
) -> u8 {
    let _ = write_diagnostic(stdout, stderr, format, &diagnostic);
    match diagnostic.category {
        DiagnosticCategory::Syntax
        | DiagnosticCategory::Capability
        | DiagnosticCategory::Configuration => 2,
        DiagnosticCategory::Compatibility => 3,
        _ => 1,
    }
}

fn config_errors(errors: Vec<ConfigDiagnostic>) -> Diagnostic {
    let message = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    Diagnostic::error("E_CONFIG", DiagnosticCategory::Configuration, message)
}

fn config_error(error: ConfigDiagnostic) -> Diagnostic {
    config_errors(vec![error])
}
fn io_error(path: &Path, error: io::Error) -> Diagnostic {
    Diagnostic::error(
        "E_IO",
        DiagnosticCategory::External,
        format!("`{}`: {error}", path.display()),
    )
}
fn output_error(error: io::Error) -> Diagnostic {
    Diagnostic::error("E_OUTPUT", DiagnosticCategory::External, error.to_string())
}
fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}
fn default_backup(paths: &ConfigPaths, database: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let name = database.file_name().unwrap_or_default().to_string_lossy();
    paths.backup_dir.join(format!("{name}.{stamp}.bak"))
}

/// @brief 把生效配置映射为与配置层解耦的 SQLite 选项。 / Map effective configuration to configuration-independent SQLite options.
/// @param config 生效数据库配置。 / Effective database configuration.
/// @return 强类型 SQLite 打开选项。 / Strongly typed SQLite open options.
fn sqlite_options(config: &DatabaseConfig) -> SqliteOptions {
    SqliteOptions::from(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn clap_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn help_is_user_facing_and_never_exposes_doxygen_markers() {
        let error = Cli::try_parse_from(["promptr", "--help"]).unwrap_err();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            write_clap_error(error, OutputFormat::Human, &mut stdout, &mut stderr),
            0
        );
        let help = String::from_utf8(stdout).unwrap();
        assert!(help.contains("Manage, compose, search, and render prompt catalogs"));
        for option in ["--color", "--glyphs", "--theme", "--preview", "--mouse"] {
            assert!(help.contains(option), "help omitted {option}");
        }
        assert!(!help.contains("@brief"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn explicit_ui_flags_win_and_record_cli_provenance() {
        use crate::infrastructure::config::{ColorMode, ConfigSource, GlyphMode, PreviewMode};

        let directory = tempfile::tempdir().unwrap();
        let paths = ConfigPaths {
            user_config: directory.path().join("config.toml"),
            database: directory.path().join("catalog.sqlite"),
            state_dir: directory.path().join("state"),
            cache_dir: directory.path().join("cache"),
            backup_dir: directory.path().join("backup"),
        };
        let explicit = directory.path().join("explicit.toml");
        fs::write(
            &explicit,
            "schema_version=1\n[ui]\ncolor='none'\nglyphs='unicode'\ntheme='dark'\nmouse=true\npreview='tree'\n",
        )
        .unwrap();
        let cli = Cli::try_parse_from(vec![
            OsString::from("promptr"),
            OsString::from("--config"),
            explicit.clone().into_os_string(),
            OsString::from("--color"),
            OsString::from("truecolor"),
            OsString::from("--glyphs"),
            OsString::from("ascii"),
            OsString::from("--theme"),
            OsString::from("light"),
            OsString::from("--preview"),
            OsString::from("metadata"),
            OsString::from("--mouse"),
            OsString::from("false"),
            OsString::from("config"),
            OsString::from("show"),
        ])
        .unwrap();
        let loaded = load_config(&cli, &paths).unwrap();

        assert_eq!(loaded.config.ui.color, ColorMode::Truecolor);
        assert_eq!(loaded.config.ui.glyphs, GlyphMode::Ascii);
        assert_eq!(loaded.config.ui.theme, "light");
        assert_eq!(loaded.config.ui.preview, PreviewMode::Metadata);
        assert!(!loaded.config.ui.mouse);
        for key in [
            "ui.color",
            "ui.glyphs",
            "ui.theme",
            "ui.preview",
            "ui.mouse",
        ] {
            let provenance = loaded.provenance.get(key).unwrap();
            assert!(matches!(&provenance.source, ConfigSource::Cli(_)));
            assert!(provenance.overridden.iter().any(
                |source| matches!(source, ConfigSource::ExplicitFile(path) if path == &explicit)
            ));
        }
    }

    #[test]
    fn malformed_json_invocation_emits_one_json_document_on_stdout() {
        for arguments in [
            vec!["promptr", "--format", "json", "--not-an-option"],
            vec!["promptr", "--format=json", "--not-an-option"],
        ] {
            let raw = arguments.iter().map(OsString::from).collect::<Vec<_>>();
            let format = early_output_format(&raw);
            let error = Cli::try_parse_from(arguments).unwrap_err();
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            assert_eq!(write_clap_error(error, format, &mut stdout, &mut stderr), 2);
            assert!(stderr.is_empty());
            let document: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
            assert_eq!(document["schema_version"], 1);
            assert_eq!(document["ok"], false);
            assert_eq!(document["diagnostics"][0]["code"], "E_CLI_PARSE");
        }
    }

    #[test]
    fn parses_nested_maintenance_commands() {
        let cli =
            Cli::try_parse_from(["promptr", "--format", "json", "db", "check", "--full"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Db(DbArgs {
                command: DbCommand::Check { full: true }
            }))
        ));
    }

    #[test]
    fn eval_preserves_bang_as_dsl_instead_of_shell() {
        let cli = Cli::try_parse_from(["promptr", "eval", "! echo nope"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Eval { source }) if source.starts_with('!')));
    }

    #[test]
    fn json_eval_runs_through_composition_root() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("catalog.sqlite");
        let cli = Cli::try_parse_from([
            "promptr",
            "--no-config",
            "--database",
            database.to_str().unwrap(),
            "--format",
            "json",
            "eval",
            "LIST;",
        ])
        .unwrap();
        let mut input = io::Cursor::new(Vec::<u8>::new());
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        assert_eq!(run(cli, &mut input, &mut stdout, &mut stderr), 0);
        assert!(stderr.is_empty());
        let document: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(document["schema_version"], 1);
        assert_eq!(document["ok"], true);
    }

    #[test]
    fn check_compiles_interactive_and_mutating_statements_without_executing_them() {
        use crate::application::ports::CatalogRead;

        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("catalog.sqlite");
        let script_path = directory.path().join("check.prompt");
        fs::write(
            &script_path,
            "FRAGMENT Draft; RENAME Draft TO Renamed; OUTPUT Renamed;",
        )
        .unwrap();
        let cli = Cli::try_parse_from([
            "promptr",
            "--no-config",
            "--database",
            database_path.to_str().unwrap(),
            "check",
            script_path.to_str().unwrap(),
        ])
        .unwrap();
        let mut input = io::Cursor::new(Vec::<u8>::new());
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());

        assert_eq!(run(cli, &mut input, &mut stdout, &mut stderr), 0);
        assert!(stderr.is_empty());
        let database = SqliteDatabase::open(&database_path).unwrap();
        assert!(database.snapshot().unwrap().is_empty());
    }

    #[test]
    fn run_dash_reads_dsl_from_standard_input() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("catalog.sqlite");
        let cli = Cli::try_parse_from([
            "promptr",
            "--no-config",
            "--database",
            database_path.to_str().unwrap(),
            "--format",
            "json",
            "run",
            "-",
        ])
        .unwrap();
        let mut input = io::Cursor::new(b"LIST;".to_vec());
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        assert_eq!(run(cli, &mut input, &mut stdout, &mut stderr), 0);
        assert!(stderr.is_empty());
        let document: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(document["ok"], true);
    }

    #[test]
    fn repl_incomplete_eof_is_published_once_without_outer_wrapper() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("catalog.sqlite");
        let cli = Cli::try_parse_from([
            "promptr",
            "--no-config",
            "--database",
            database_path.to_str().unwrap(),
        ])
        .unwrap();
        let mut input = io::Cursor::new(b"Root: [".to_vec());
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        assert_eq!(run(cli, &mut input, &mut stdout, &mut stderr), 2);
        let errors = String::from_utf8(stderr).unwrap();
        assert_eq!(errors.matches("E0004").count(), 1);
        assert!(!errors.contains("E_REPL"));
    }

    #[test]
    fn tui_requires_human_mode_without_a_subcommand_and_two_ttys() {
        let interactive = Cli::try_parse_from(["promptr"]).unwrap();
        assert!(should_use_tui(&interactive, true, true));
        assert!(!should_use_tui(&interactive, false, true));
        assert!(!should_use_tui(&interactive, true, false));

        let json = Cli::try_parse_from(["promptr", "--format", "json"]).unwrap();
        assert!(!should_use_tui(&json, true, true));
        let command = Cli::try_parse_from(["promptr", "eval", "LIST;"]).unwrap();
        assert!(!should_use_tui(&command, true, true));
    }

    #[test]
    fn raw_mixed_program_is_rejected_before_mutation() {
        use crate::{
            application::{Value, ports::Database},
            domain::{Symbol, XmlText},
        };

        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("catalog.sqlite");
        let mut database = SqliteDatabase::open(&database_path).unwrap();
        database
            .write_transaction(&mut |writer| {
                writer.upsert_fragment(
                    &Symbol::new("Leaf").unwrap(),
                    &XmlText::new("original").unwrap(),
                    None,
                )?;
                Ok(Vec::<Value>::new())
            })
            .unwrap();
        let leaf = Symbol::new("Leaf").unwrap();
        let before = crate::application::ports::CatalogRead::snapshot(&database)
            .unwrap()
            .get_by_symbol(&leaf)
            .unwrap()
            .clone();
        drop(database);

        let cli = Cli::try_parse_from([
            "promptr",
            "--no-config",
            "--database",
            database_path.to_str().unwrap(),
            "--format",
            "raw",
            "eval",
            "METADATA Leaf DESCRIPTION \"changed\"; OUTPUT Leaf;",
        ])
        .unwrap();
        let mut input = io::Cursor::new(Vec::<u8>::new());
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        assert_eq!(run(cli, &mut input, &mut stdout, &mut stderr), 2);
        assert!(stdout.is_empty());

        let database = SqliteDatabase::open(&database_path).unwrap();
        let after = crate::application::ports::CatalogRead::snapshot(&database)
            .unwrap()
            .get_by_symbol(&leaf)
            .unwrap()
            .clone();
        assert_eq!(before, after);
    }

    #[test]
    fn maintenance_commands_do_not_create_a_missing_database() {
        let directory = tempfile::tempdir().unwrap();
        for arguments in [["db", "status"], ["db", "check"]] {
            let database = directory.path().join(format!("{}.sqlite", arguments[1]));
            let cli = Cli::try_parse_from([
                "promptr",
                "--no-config",
                "--database",
                database.to_str().unwrap(),
                arguments[0],
                arguments[1],
            ])
            .unwrap();
            let mut input = io::Cursor::new(Vec::<u8>::new());
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            assert_ne!(run(cli, &mut input, &mut stdout, &mut stderr), 0);
            assert!(!database.exists());
        }
    }
}
