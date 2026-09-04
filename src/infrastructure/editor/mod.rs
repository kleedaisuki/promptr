//! 文本编辑器适配器。 / Text editor adapters.
//!
//! 编辑器只负责在内存中准备文本；它不持有存储引用，也不执行任何持久化操作。
//! Editors only prepare text in memory; they neither hold a store reference nor persist changes.

mod builtin;
mod external;

use crate::domain::{NodeId, Revision};
use std::ffi::OsString;

pub use builtin::{BuiltinEditorHost, BuiltinTextProvider};
pub use external::ExternalTextProvider;

/// 编辑器错误。 / Errors produced by editor adapters.
///
/// <!-- @brief 编辑器错误。 / Editor error. -->
#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    /// 外部编辑器命令为空。 / The external editor command is empty.
    ///
    /// <!-- @brief 外部编辑器命令为空。 / The external editor command is empty. -->
    #[error("external editor command must contain a program")]
    EmptyCommand,

    /// `{file}` 占位符数量不是一。 / The `{file}` placeholder count is not one.
    ///
    /// <!-- @brief `{file}` 占位符数量不是一。 / The `{file}` placeholder count is not one. -->
    #[error("external editor argv must contain exactly one `{{file}}` argument; found {found}")]
    InvalidFilePlaceholder {
        /// 实际占位符数量。 / Actual placeholder count.
        ///
        /// <!-- @brief 实际占位符数量。 / Actual placeholder count. -->
        found: usize,
    },

    /// 编辑器 I/O 操作失败。 / An editor I/O operation failed.
    ///
    /// <!-- @brief 编辑器 I/O 操作失败。 / An editor I/O operation failed. -->
    #[error("editor {operation} failed: {source}")]
    Io {
        /// 失败的操作名称。 / Name of the failed operation.
        ///
        /// <!-- @brief 失败的操作名称。 / Name of the failed operation. -->
        operation: &'static str,
        /// 底层 I/O 错误。 / Underlying I/O error.
        ///
        /// <!-- @brief 底层 I/O 错误。 / Underlying I/O error. -->
        #[source]
        source: std::io::Error,
    },

    /// 外部编辑器以失败状态退出。 / The external editor exited unsuccessfully.
    ///
    /// <!-- @brief 外部编辑器以失败状态退出。 / The external editor exited unsuccessfully. -->
    #[error("external editor exited unsuccessfully (code: {code:?})")]
    ExternalFailure {
        /// 进程退出码；被信号终止时为空。 /
        /// Process exit code, or none when terminated by a signal.
        ///
        /// <!-- @brief 进程退出码；被信号终止时为空。 / Process exit code, or none when terminated by a signal. -->
        code: Option<i32>,
    },
}

/// 一次编辑的乐观并发标识。 / Optimistic identity for one edit.
///
/// <!-- @brief 一次编辑的乐观并发标识。 / Optimistic identity for one edit. -->
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditTarget {
    /// 现有节点标识；新建节点为空。 / Existing node identity; none for a new node.
    ///
    /// <!-- @brief 现有节点标识；新建节点为空。 / Existing node identity; none for a new node. -->
    pub node_id: Option<NodeId>,
    /// 编辑开始时的节点修订；新建节点为空。 / Node revision at edit start; none for a new node.
    ///
    /// <!-- @brief 编辑开始时的节点修订；新建节点为空。 / Node revision at edit start; none for a new node. -->
    pub base_revision: Option<Revision>,
}

/// 发送给文本提供者的编辑请求。 / Edit request sent to a text provider.
///
/// <!-- @brief 发送给文本提供者的编辑请求。 / Edit request sent to a text provider. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditRequest {
    /// 编辑目标与基础修订。 / Edit target and base revision.
    ///
    /// <!-- @brief 编辑目标与基础修订。 / Edit target and base revision. -->
    pub target: EditTarget,
    /// 编辑前的精确文本。 / Exact text before editing.
    ///
    /// <!-- @brief 编辑前的精确文本。 / Exact text before editing. -->
    pub original_text: String,
}

/// 在写事务之前准备好的编辑。 / An edit prepared before a write transaction.
///
/// <!-- @brief 在写事务之前准备好的编辑。 / An edit prepared before a write transaction. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedEdit {
    /// 编辑目标与基础修订。 / Edit target and base revision.
    ///
    /// <!-- @brief 编辑目标与基础修订。 / Edit target and base revision. -->
    pub target: EditTarget,
    /// 编辑前的精确文本。 / Exact text before editing.
    ///
    /// <!-- @brief 编辑前的精确文本。 / Exact text before editing. -->
    pub original_text: String,
    /// 用户保存的精确文本。 / Exact text saved by the user.
    ///
    /// <!-- @brief 用户保存的精确文本。 / Exact text saved by the user. -->
    pub edited_text: String,
}

/// 编辑会话的结果。 / Outcome of an editing session.
///
/// <!-- @brief 编辑会话的结果。 / Outcome of an editing session. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditorOutcome {
    /// 显式保存的预备编辑。 / Explicitly saved prepared edit.
    ///
    /// <!-- @brief 显式保存的预备编辑。 / Explicitly saved prepared edit. -->
    Save(PreparedEdit),
    /// 取消且不产生可持久化的文本。 / Cancellation with no persistable text.
    ///
    /// <!-- @brief 取消且不产生可持久化的文本。 / Cancellation with no persistable text. -->
    Cancel,
}

/// 文本编辑能力边界。 / Text-editing capability boundary.
///
/// <!-- @brief 文本编辑能力边界。 / Text-editing capability boundary. -->
pub trait TextProvider {
    /// 编辑请求文本。 / Edits the requested text.
    ///
    /// # Arguments
    ///
    /// * `request` - 包含乐观标识和原文的请求。 /
    ///   Request containing optimistic identity and original text.
    ///
    /// # Returns
    ///
    /// 保存或取消的结果。 / The saved or cancelled outcome.
    ///
    /// # Errors
    ///
    /// 创建或读写暂存文件失败、宿主编辑会话失败，或外部编辑器启动或退出失败时返回
    /// [`EditorError`]。 / Returns [`EditorError`] when staging-file I/O, the host editing session,
    /// external process launch, or external process completion fails.
    ///
    /// # Notes
    ///
    /// 实现不得持有存储句柄。 / Implementations must not hold a store handle.
    ///
    /// <!-- @brief 编辑请求文本。 / Edit the requested text. -->
    /// <!-- @param request 包含乐观标识和原文的请求。 / Request containing optimistic identity and original text. -->
    /// <!-- @return 保存、取消或可观测错误。 / Save, cancellation, or an observable error. -->
    /// <!-- @note 实现不得持有存储句柄。 / Implementations must not hold a store handle. -->
    fn edit(&mut self, request: EditRequest) -> Result<EditorOutcome, EditorError>;
}

/// 编辑器选择策略。 / Editor selection strategy.
///
/// <!-- @brief 编辑器选择策略。 / Editor selection strategy. -->
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditorStrategy {
    /// 由宿主 TUI 实现的内置编辑器。 / Built-in editor implemented by the host TUI.
    ///
    /// <!-- @brief 由宿主 TUI 实现的内置编辑器。 / Built-in editor implemented by the host TUI. -->
    Builtin,
    /// 不经 shell 执行的外部 argv。 / External argv executed without a shell.
    ///
    /// <!-- @brief 不经 shell 执行的外部 argv。 / External argv executed without a shell. -->
    External(Vec<OsString>),
}

impl Default for EditorStrategy {
    /// 返回零配置的内置编辑器策略。 / Returns the zero-configuration built-in editor strategy.
    ///
    /// # Returns
    ///
    /// 内置编辑器策略。 / The built-in editor strategy.
    ///
    /// <!-- @brief 返回零配置的内置编辑器策略。 / Return the zero-configuration built-in editor strategy. -->
    /// <!-- @return 内置编辑器策略。 / Built-in editor strategy. -->
    fn default() -> Self {
        Self::Builtin
    }
}

impl PreparedEdit {
    /// 从请求和已保存文本构建预备编辑。 /
    /// Builds a prepared edit from a request and saved text.
    ///
    /// # Arguments
    ///
    /// * `request` - 原始编辑请求。 / Original edit request.
    /// * `edited_text` - 用户保存的文本。 / Text saved by the user.
    ///
    /// # Returns
    ///
    /// 保留乐观标识的预备编辑。 / A prepared edit preserving optimistic identity.
    ///
    /// <!-- @brief 从请求和已保存文本构建预备编辑。 / Build a prepared edit from a request and saved text. -->
    /// <!-- @param request 原始编辑请求。 / Original edit request. -->
    /// <!-- @param edited_text 用户保存的文本。 / Text saved by the user. -->
    /// <!-- @return 保留乐观标识的预备编辑。 / Prepared edit preserving optimistic identity. -->
    pub fn from_request(request: EditRequest, edited_text: String) -> Self {
        Self {
            target: request.target,
            original_text: request.original_text,
            edited_text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EditorStrategy;

    #[test]
    fn builtin_is_the_default_strategy() {
        assert_eq!(EditorStrategy::default(), EditorStrategy::Builtin);
    }
}
