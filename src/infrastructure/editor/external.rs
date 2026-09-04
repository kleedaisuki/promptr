//! 外部编辑器进程适配器。 / External editor process adapter.

use super::{EditRequest, EditorError, EditorOutcome, PreparedEdit, TextProvider};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::Builder;

/// 外部编辑器 argv 中的暂存文件占位符。 / Staging-file placeholder in external editor argv.
///
/// <!-- @brief 外部编辑器 argv 中的暂存文件占位符。 / Staging-file placeholder in external editor argv. -->
const FILE_PLACEHOLDER: &str = "{file}";

/// 通过类型化 argv 启动的外部编辑器。 / External editor launched through typed argv.
///
/// <!-- @brief 通过类型化 argv 启动的外部编辑器。 / External editor launched through typed argv. -->
#[derive(Clone, Debug)]
pub struct ExternalTextProvider {
    /// 程序名与参数，其中恰有一个 `{file}` 参数。 /
    /// Program and arguments containing exactly one `{file}` argument.
    ///
    /// <!-- @brief 程序名与参数，其中恰有一个 `{file}` 参数。 / Program and arguments containing exactly one `{file}` argument. -->
    argv: Vec<OsString>,
}

impl ExternalTextProvider {
    /// 创建不经 shell 的外部编辑器提供者。 /
    /// Creates an external editor provider that bypasses the shell.
    ///
    /// # Arguments
    ///
    /// * `argv` - 程序名和类型化参数；`{file}` 必须恰好是一个完整参数。 /
    ///   Program and typed arguments; `{file}` must be exactly one complete argument.
    ///
    /// # Returns
    ///
    /// 已验证的提供者。 / The validated provider.
    ///
    /// # Errors
    ///
    /// 命令为空时返回 [`EditorError::EmptyCommand`]；`{file}` 占位符不是恰好一个完整参数时
    /// 返回 [`EditorError::InvalidFilePlaceholder`]。 / Returns [`EditorError::EmptyCommand`] for an
    /// empty command and [`EditorError::InvalidFilePlaceholder`] unless `{file}` occurs exactly once
    /// as a complete argument.
    ///
    /// # Notes
    ///
    /// 不执行 shell 展开或字符串拼接。 / No shell expansion or command-string concatenation is performed.
    ///
    /// <!-- @brief 创建不经 shell 的外部编辑器提供者。 / Create an external editor provider that bypasses the shell. -->
    /// <!-- @param argv 程序名和类型化参数；`{file}` 必须恰好是一个完整参数。 / Program and typed arguments; `{file}` must be exactly one complete argument. -->
    /// <!-- @return 已验证的提供者，或配置错误。 / Validated provider or a configuration error. -->
    /// <!-- @note 不执行 shell 展开或字符串拼接。 / No shell expansion or command-string concatenation is performed. -->
    pub fn new<I, S>(argv: I) -> Result<Self, EditorError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let argv = argv.into_iter().map(Into::into).collect::<Vec<_>>();
        if argv.is_empty() {
            return Err(EditorError::EmptyCommand);
        }

        let found = argv
            .iter()
            .skip(1)
            .filter(|argument| argument.as_os_str() == OsStr::new(FILE_PLACEHOLDER))
            .count();
        if found != 1 {
            return Err(EditorError::InvalidFilePlaceholder { found });
        }

        Ok(Self { argv })
    }

    /// 返回原始类型化 argv。 / Returns the original typed argv.
    ///
    /// # Returns
    ///
    /// 未替换 `{file}` 的 argv。 / The argv with `{file}` still unexpanded.
    ///
    /// <!-- @brief 返回原始类型化 argv。 / Return the original typed argv. -->
    /// <!-- @return 未替换 `{file}` 的 argv。 / Argv with `{file}` still unexpanded. -->
    pub fn argv(&self) -> &[OsString] {
        &self.argv
    }

    /// 为指定暂存路径构建进程命令。 / Builds a process command for a staging path.
    ///
    /// # Arguments
    ///
    /// * `path` - 替换 `{file}` 的原生平台路径。 / Native platform path replacing `{file}`.
    ///
    /// # Returns
    ///
    /// 不经 shell 的进程命令。 / A process command that bypasses the shell.
    ///
    /// <!-- @brief 为指定暂存路径构建进程命令。 / Build a process command for a staging path. -->
    /// <!-- @param path 替换 `{file}` 的原生平台路径。 / Native platform path replacing `{file}`. -->
    /// <!-- @return 不经 shell 的进程命令。 / Process command that bypasses the shell. -->
    fn command_for(&self, path: &Path) -> Command {
        let mut command = Command::new(&self.argv[0]);
        command.args(self.argv.iter().skip(1).map(|argument| {
            if argument.as_os_str() == OsStr::new(FILE_PLACEHOLDER) {
                path.as_os_str()
            } else {
                argument.as_os_str()
            }
        }));
        command
    }
}

impl TextProvider for ExternalTextProvider {
    /// 在暂存文件上运行已配置的外部编辑器。 /
    /// Runs the configured external editor against a staging file.
    ///
    /// <!-- @brief 在暂存文件上运行已配置的外部编辑器。 / Run the configured external editor against a staging file. -->
    /// <!-- @param request 包含原文与并发标识的编辑请求。 / Edit request containing original text and concurrency identity. -->
    /// <!-- @return 已保存的预备编辑或可观测错误。 / Saved prepared edit or an observable error. -->
    fn edit(&mut self, request: EditRequest) -> Result<EditorOutcome, EditorError> {
        let mut staging = Builder::new()
            .prefix("promptr-edit-")
            .suffix(".txt")
            .tempfile()
            .map_err(|source| EditorError::Io {
                operation: "staging-file creation",
                source,
            })?;
        staging
            .write_all(request.original_text.as_bytes())
            .and_then(|()| staging.flush())
            .map_err(|source| EditorError::Io {
                operation: "staging-file write",
                source,
            })?;

        // 启动前关闭文件，使各平台的编辑器都能重新打开它。 /
        // Close before launch so editors on every platform can reopen the file.
        let staging_path = staging.into_temp_path();
        let status = self
            .command_for(staging_path.as_ref())
            .status()
            .map_err(|source| EditorError::Io {
                operation: "process launch",
                source,
            })?;
        if !status.success() {
            return Err(EditorError::ExternalFailure {
                code: status.code(),
            });
        }

        let staging_path_ref: &Path = staging_path.as_ref();
        let edited_text =
            fs::read_to_string(staging_path_ref).map_err(|source| EditorError::Io {
                operation: "staging-file read",
                source,
            })?;
        Ok(EditorOutcome::Save(PreparedEdit::from_request(
            request,
            edited_text,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::editor::EditTarget;

    #[test]
    fn placeholder_is_replaced_as_one_typed_argument() {
        let provider = ExternalTextProvider::new([
            OsString::from("editor"),
            OsString::from("--wait"),
            OsString::from(FILE_PLACEHOLDER),
        ])
        .unwrap();
        let command = provider.command_for(Path::new("a path/draft.txt"));
        let args = command.get_args().collect::<Vec<_>>();
        assert_eq!(args, [OsStr::new("--wait"), OsStr::new("a path/draft.txt")]);
    }

    #[test]
    fn rejects_missing_or_repeated_placeholder() {
        assert!(matches!(
            ExternalTextProvider::new(["editor"]),
            Err(EditorError::InvalidFilePlaceholder { found: 0 })
        ));
        assert!(matches!(
            ExternalTextProvider::new(["editor", FILE_PLACEHOLDER, FILE_PLACEHOLDER]),
            Err(EditorError::InvalidFilePlaceholder { found: 2 })
        ));
    }

    #[cfg(windows)]
    fn powershell_provider(script: &Path) -> ExternalTextProvider {
        ExternalTextProvider::new([
            OsString::from("powershell.exe"),
            OsString::from("-NoProfile"),
            OsString::from("-NonInteractive"),
            OsString::from("-File"),
            script.as_os_str().to_owned(),
            OsString::from(FILE_PLACEHOLDER),
        ])
        .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn external_content_round_trips_and_failure_never_saves() {
        let directory = tempfile::tempdir().unwrap();
        let success_script = directory.path().join("edit.ps1");
        fs::write(
            &success_script,
            "param([string]$Path)\n[IO.File]::WriteAllText($Path, \"edited``content\", [Text.UTF8Encoding]::new($false))\n",
        )
        .unwrap();
        let mut provider = powershell_provider(&success_script);
        let request = EditRequest {
            target: EditTarget {
                node_id: None,
                base_revision: None,
            },
            original_text: "original".into(),
        };
        let EditorOutcome::Save(edit) = provider.edit(request.clone()).unwrap() else {
            panic!("expected save")
        };
        assert_eq!(edit.edited_text, "edited`content");

        let failure_script = directory.path().join("fail.ps1");
        fs::write(&failure_script, "param([string]$Path)\nexit 7\n").unwrap();
        let mut provider = powershell_provider(&failure_script);
        assert!(matches!(
            provider.edit(request),
            Err(EditorError::ExternalFailure { code: Some(7) })
        ));

        let invalid_script = directory.path().join("invalid.ps1");
        fs::write(
            &invalid_script,
            "param([string]$Path)\n[IO.File]::WriteAllBytes($Path, [byte[]](0xff, 0xfe))\n",
        )
        .unwrap();
        let mut provider = powershell_provider(&invalid_script);
        let request = EditRequest {
            target: EditTarget {
                node_id: None,
                base_revision: None,
            },
            original_text: "original".into(),
        };
        assert!(matches!(
            provider.edit(request),
            Err(EditorError::Io { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn external_content_round_trips_and_failure_never_saves() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let success_script = directory.path().join("edit");
        fs::write(
            &success_script,
            "#!/bin/sh\nprintf 'edited content' > \"$1\"\n",
        )
        .unwrap();
        fs::set_permissions(&success_script, fs::Permissions::from_mode(0o700)).unwrap();
        let mut provider = ExternalTextProvider::new([
            success_script.as_os_str().to_owned(),
            OsString::from(FILE_PLACEHOLDER),
        ])
        .unwrap();
        let request = EditRequest {
            target: EditTarget {
                node_id: None,
                base_revision: None,
            },
            original_text: "original".into(),
        };
        let EditorOutcome::Save(edit) = provider.edit(request.clone()).unwrap() else {
            panic!("expected save")
        };
        assert_eq!(edit.edited_text, "edited content");

        let failure_script = directory.path().join("fail");
        fs::write(&failure_script, "#!/bin/sh\nexit 7\n").unwrap();
        fs::set_permissions(&failure_script, fs::Permissions::from_mode(0o700)).unwrap();
        let mut provider = ExternalTextProvider::new([
            failure_script.as_os_str().to_owned(),
            OsString::from(FILE_PLACEHOLDER),
        ])
        .unwrap();
        assert!(matches!(
            provider.edit(request),
            Err(EditorError::ExternalFailure { code: Some(7) })
        ));

        let invalid_script = directory.path().join("invalid");
        fs::write(&invalid_script, "#!/bin/sh\nprintf '\\377\\376' > \"$1\"\n").unwrap();
        fs::set_permissions(&invalid_script, fs::Permissions::from_mode(0o700)).unwrap();
        let mut provider = ExternalTextProvider::new([
            invalid_script.as_os_str().to_owned(),
            OsString::from(FILE_PLACEHOLDER),
        ])
        .unwrap();
        let request = EditRequest {
            target: EditTarget {
                node_id: None,
                base_revision: None,
            },
            original_text: "original".into(),
        };
        assert!(matches!(
            provider.edit(request),
            Err(EditorError::Io { .. })
        ));
    }
}
