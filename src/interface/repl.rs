//! 交互式命令宿主。 / Interactive command host.

use std::{
    io::{BufRead, Write},
    process::Command,
};

use crate::{
    Diagnostic, DiagnosticCategory, InvocationPolicy, Promptr,
    infrastructure::{
        config::{EditorConfig, EditorMode},
        editor::{
            EditRequest, EditorError, EditorOutcome, ExternalTextProvider, PreparedEdit,
            TextProvider,
        },
    },
    language::{ParseOutcome, parse},
};

use super::presenter::{OutputFormat, write_diagnostic, write_values};

/// @brief REPL 的类型化终止结果。 / Typed REPL termination outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplOutcome {
    /// @brief 用户正常结束会话。 / The user ended the session normally.
    Completed,
    /// @brief 输入在未完成语句中结束。 / Input ended in an incomplete statement.
    IncompleteInput,
    /// @brief 读取输入失败。 / Reading input failed.
    InputIo,
    /// @brief 写入交互输出失败。 / Writing interactive output failed.
    OutputIo,
}

impl ReplOutcome {
    /// @brief 映射为稳定的进程退出码。 / Map to the stable process exit code.
    /// @return 成功为 0，I/O 失败为 1，不完整输入为 2。 / Zero for success, one for I/O failure, and two for incomplete input.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Completed => 0,
            Self::InputIo | Self::OutputIo => 1,
            Self::IncompleteInput => 2,
        }
    }
}

/// @brief 运行逐语句 REPL。 / Run the statement-oriented REPL.
/// @param app 已装配应用门面。 / Assembled application facade.
/// @param input 输入流。 / Input stream.
/// @param stdout 标准输出。 / Standard output.
/// @param stderr 标准错误。 / Standard error.
/// @return 类型化终止结果；致命诊断已由 REPL 精确发布一次。 / Typed termination outcome; a fatal diagnostic has already been published exactly once by the REPL.
pub fn run(
    app: &mut Promptr,
    input: &mut dyn BufRead,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> ReplOutcome {
    let mut source = String::new();
    loop {
        if write!(
            stdout,
            "{}",
            if source.is_empty() {
                "promptr> "
            } else {
                "......> "
            }
        )
        .and_then(|()| stdout.flush())
        .is_err()
        {
            return ReplOutcome::OutputIo;
        }
        let mut line = String::new();
        match input.read_line(&mut line) {
            Ok(0) => {
                if !source.is_empty() {
                    let diagnostic = Diagnostic::error(
                        "E0004",
                        DiagnosticCategory::Syntax,
                        "input ended while a statement was incomplete",
                    );
                    let _ = write_diagnostic(stdout, stderr, OutputFormat::Human, &diagnostic);
                    return ReplOutcome::IncompleteInput;
                }
                let _ = writeln!(stdout);
                return ReplOutcome::Completed;
            }
            Ok(_) => {}
            Err(error) => {
                let diagnostic = Diagnostic::error(
                    "E_REPL_IO",
                    DiagnosticCategory::External,
                    "failed to read interactive input",
                )
                .with_cause(error.to_string());
                let _ = write_diagnostic(stdout, stderr, OutputFormat::Human, &diagnostic);
                return ReplOutcome::InputIo;
            }
        }

        if source.is_empty() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if matches!(trimmed, ":quit" | ":q" | "quit" | "exit") {
                return ReplOutcome::Completed;
            }
            if let Some(command) = line.strip_prefix('!') {
                if let Err(diagnostic) = execute_shell(command.trim_end()) {
                    let _ = write_diagnostic(stdout, stderr, OutputFormat::Human, &diagnostic);
                }
                continue;
            }
        }

        source.push_str(&line);
        match parse(&source) {
            ParseOutcome::Incomplete(_) => continue,
            ParseOutcome::Invalid(diagnostic) => {
                let diagnostic = Diagnostic::error(
                    diagnostic.code.as_str(),
                    DiagnosticCategory::Syntax,
                    diagnostic.message,
                );
                let _ = write_diagnostic(stdout, stderr, OutputFormat::Human, &diagnostic);
            }
            ParseOutcome::Complete(_) => {
                let result = match provider_strategy(&app.config().editor) {
                    Ok(ProviderStrategy::Builtin) => {
                        let mut provider = ReplTextProvider { input, stdout };
                        app.eval_with_provider(
                            &source,
                            InvocationPolicy::interactive(),
                            &mut provider,
                        )
                    }
                    Ok(ProviderStrategy::External(argv)) => match ExternalTextProvider::new(argv) {
                        Ok(mut provider) => app.eval_with_provider(
                            &source,
                            InvocationPolicy::interactive(),
                            &mut provider,
                        ),
                        Err(error) => Err(editor_diagnostic(error)),
                    },
                    Err(diagnostic) => Err(diagnostic),
                };
                match result {
                    Ok(values) => {
                        let _ = write_values(stdout, OutputFormat::Human, &values);
                    }
                    Err(diagnostic) => {
                        let _ = write_diagnostic(stdout, stderr, OutputFormat::Human, &diagnostic);
                    }
                }
            }
        }
        source.clear();
    }
}

/// @brief REPL 使用的编辑器提供者策略。 / Editor-provider strategy used by the REPL.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ProviderStrategy {
    /// @brief 使用 REPL 内建行编辑器。 / Use the built-in REPL line editor.
    Builtin,
    /// @brief 直接启动配置的外部 argv。 / Directly launch the configured external argv.
    External(Vec<String>),
}

/// @brief 把生效编辑器配置映射为 REPL 策略。 / Map effective editor configuration to a REPL strategy.
/// @param config 已验证编辑器配置。 / Validated editor configuration.
/// @return 编辑器策略或配置诊断。 / Editor strategy or a configuration diagnostic.
fn provider_strategy(config: &EditorConfig) -> Result<ProviderStrategy, Diagnostic> {
    match config.mode {
        EditorMode::Builtin => Ok(ProviderStrategy::Builtin),
        EditorMode::External => config
            .external
            .clone()
            .map(ProviderStrategy::External)
            .ok_or_else(|| {
                Diagnostic::error(
                    "E_EDITOR_CONFIG",
                    DiagnosticCategory::Configuration,
                    "external editor mode requires a configured argv",
                )
            }),
    }
}

/// @brief 把编辑器适配错误转换为共享诊断。 / Convert an editor-adapter error to a shared diagnostic.
/// @param error 编辑器错误。 / Editor error.
/// @return 结构化外部系统诊断。 / Structured external-system diagnostic.
fn editor_diagnostic(error: EditorError) -> Diagnostic {
    Diagnostic::error(
        "E_EDITOR",
        DiagnosticCategory::External,
        "could not prepare the configured editor",
    )
    .with_cause(error.to_string())
}

/// @brief REPL 的无存储行编辑适配器。 / Store-free line editor adapter for the REPL.
struct ReplTextProvider<'a> {
    /// @brief 与命令循环共享的输入。 / Input shared with the command loop.
    input: &'a mut dyn BufRead,
    /// @brief 编辑提示与原文预览目标。 / Destination for edit prompts and original-text preview.
    stdout: &'a mut dyn Write,
}

impl TextProvider for ReplTextProvider<'_> {
    /// @brief 读取多行直到显式保存或取消。 / Read lines until an explicit save or cancellation.
    /// @param request 原文与乐观身份。 / Original text and optimistic identity.
    /// @return 保存的精确正文或取消。 / Exact saved body or cancellation.
    fn edit(&mut self, request: EditRequest) -> Result<EditorOutcome, EditorError> {
        writeln!(
            self.stdout,
            "-- fragment editor (.save saves, .cancel cancels); current text follows --"
        )
        .and_then(|()| {
            if request.original_text.is_empty() {
                Ok(())
            } else {
                self.stdout.write_all(request.original_text.as_bytes())?;
                if request.original_text.ends_with('\n') {
                    Ok(())
                } else {
                    writeln!(self.stdout)
                }
            }
        })
        .and_then(|()| self.stdout.flush())
        .map_err(|source| EditorError::Io {
            operation: "write REPL editor prompt",
            source,
        })?;

        let mut edited = String::new();
        loop {
            let mut line = String::new();
            let count = self
                .input
                .read_line(&mut line)
                .map_err(|source| EditorError::Io {
                    operation: "read REPL editor input",
                    source,
                })?;
            if count == 0 || line.trim_end_matches(['\r', '\n']) == ".cancel" {
                return Ok(EditorOutcome::Cancel);
            }
            if line.trim_end_matches(['\r', '\n']) == ".save" {
                return Ok(EditorOutcome::Save(PreparedEdit::from_request(
                    request, edited,
                )));
            }
            edited.push_str(&line);
        }
    }
}

/// @brief 使用平台 shell 执行 REPL 元命令。 / Execute a REPL meta-command with the platform shell.
/// @param source shell 命令文本。 / Shell command text.
/// @return 成功或外部进程诊断。 / Success or an external-process diagnostic.
fn execute_shell(source: &str) -> Result<(), Diagnostic> {
    if source.is_empty() {
        return Ok(());
    }
    #[cfg(windows)]
    let status = Command::new("cmd").args(["/C", source]).status();
    #[cfg(not(windows))]
    let status = Command::new("sh").args(["-c", source]).status();
    let status = status.map_err(|error| {
        Diagnostic::error(
            "E_REPL_SHELL",
            DiagnosticCategory::External,
            "could not start the platform shell",
        )
        .with_cause(error.to_string())
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(Diagnostic::error(
            "E_REPL_SHELL_STATUS",
            DiagnosticCategory::External,
            format!("shell command exited with {status}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eof_exits_cleanly_without_an_application_call() {
        let paths = crate::infrastructure::config::ConfigPaths {
            user_config: "config.toml".into(),
            database: "db.sqlite".into(),
            state_dir: "state".into(),
            cache_dir: "cache".into(),
            backup_dir: "backup".into(),
        };
        let config = crate::infrastructure::config::Config::defaults(&paths);
        let database = crate::infrastructure::sqlite::SqliteDatabase::open_in_memory().unwrap();
        let mut app = Promptr::from_parts(Box::new(database), config);
        let mut input = std::io::Cursor::new(Vec::<u8>::new());
        let (mut output, mut errors) = (Vec::new(), Vec::new());
        assert_eq!(
            run(&mut app, &mut input, &mut output, &mut errors),
            ReplOutcome::Completed
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn fragment_line_editor_saves_only_on_save_marker() {
        let paths = crate::infrastructure::config::ConfigPaths {
            user_config: "config.toml".into(),
            database: "db.sqlite".into(),
            state_dir: "state".into(),
            cache_dir: "cache".into(),
            backup_dir: "backup".into(),
        };
        let config = crate::infrastructure::config::Config::defaults(&paths);
        let database = crate::infrastructure::sqlite::SqliteDatabase::open_in_memory().unwrap();
        let mut app = Promptr::from_parts(Box::new(database), config);
        let mut input = std::io::Cursor::new(b"FRAGMENT Leaf;\nhello\n.save\n".to_vec());
        let (mut output, mut errors) = (Vec::new(), Vec::new());
        assert_eq!(
            run(&mut app, &mut input, &mut output, &mut errors),
            ReplOutcome::Completed
        );
        let values = app
            .eval("OUTPUT Leaf;", InvocationPolicy::script())
            .unwrap();
        assert!(matches!(&values[..], [crate::Value::Xml(xml)] if xml.contains("hello")));
    }

    #[test]
    fn fragment_line_editor_cancel_leaves_catalog_unchanged() {
        let paths = crate::infrastructure::config::ConfigPaths {
            user_config: "config.toml".into(),
            database: "db.sqlite".into(),
            state_dir: "state".into(),
            cache_dir: "cache".into(),
            backup_dir: "backup".into(),
        };
        let config = crate::infrastructure::config::Config::defaults(&paths);
        let database = crate::infrastructure::sqlite::SqliteDatabase::open_in_memory().unwrap();
        let mut app = Promptr::from_parts(Box::new(database), config);
        let mut input = std::io::Cursor::new(b"FRAGMENT Leaf;\n.cancel\n".to_vec());
        let (mut output, mut errors) = (Vec::new(), Vec::new());
        assert_eq!(
            run(&mut app, &mut input, &mut output, &mut errors),
            ReplOutcome::Completed
        );
        let values = app.eval("LIST;", InvocationPolicy::script()).unwrap();
        assert!(matches!(&values[..], [crate::Value::Nodes(nodes)] if nodes.is_empty()));
    }

    #[test]
    fn provider_strategy_follows_effective_editor_config() {
        let paths = crate::infrastructure::config::ConfigPaths {
            user_config: "config.toml".into(),
            database: "db.sqlite".into(),
            state_dir: "state".into(),
            cache_dir: "cache".into(),
            backup_dir: "backup".into(),
        };
        let mut config = crate::infrastructure::config::Config::defaults(&paths);
        assert_eq!(
            provider_strategy(&config.editor).unwrap(),
            ProviderStrategy::Builtin
        );
        config.editor.mode = EditorMode::External;
        config.editor.external = Some(vec!["editor".into(), "{file}".into()]);
        assert_eq!(
            provider_strategy(&config.editor).unwrap(),
            ProviderStrategy::External(vec!["editor".into(), "{file}".into()])
        );
    }
}
