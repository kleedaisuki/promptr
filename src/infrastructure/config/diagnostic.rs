//! 配置诊断 / Configuration diagnostics.

use std::fmt;

/// @brief 诊断严重级别 / Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// @brief 阻止配置生效的错误 / An error preventing activation.
    Error,
    /// @brief 不阻止生效的警告 / A non-blocking warning.
    Warning,
}

/// @brief 结构化配置诊断 / Structured configuration diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDiagnostic {
    /// @brief 稳定诊断代码 / Stable diagnostic code.
    pub code: &'static str,
    /// @brief 严重级别 / Severity.
    pub severity: DiagnosticSeverity,
    /// @brief 面向用户的消息 / User-facing message.
    pub message: String,
    /// @brief 可选的配置键 / Optional dotted configuration key.
    pub key: Option<String>,
    /// @brief 可选的源位置 / Optional source location.
    pub source: Option<String>,
    /// @brief 可操作的建议 / Optional actionable suggestion.
    pub suggestion: Option<String>,
}

impl ConfigDiagnostic {
    /// @brief 创建错误诊断 / Creates an error diagnostic.
    /// @param code 稳定代码 / Stable code.
    /// @param message 诊断文本 / Diagnostic text.
    /// @return 新诊断 / New diagnostic.
    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Error,
            message: message.into(),
            key: None,
            source: None,
            suggestion: None,
        }
    }

    /// @brief 创建警告诊断 / Creates a warning diagnostic.
    /// @param code 稳定代码 / Stable code.
    /// @param message 诊断文本 / Diagnostic text.
    /// @return 新诊断 / New diagnostic.
    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            key: None,
            source: None,
            suggestion: None,
        }
    }

    /// @brief 关联配置键 / Associates a configuration key.
    /// @param key 点分键 / Dotted key.
    /// @return 更新后的诊断 / Updated diagnostic.
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// @brief 关联源位置 / Associates a source location.
    /// @param source 源描述 / Source description.
    /// @return 更新后的诊断 / Updated diagnostic.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// @brief 添加修复建议 / Adds a repair suggestion.
    /// @param suggestion 建议文本 / Suggestion text.
    /// @return 更新后的诊断 / Updated diagnostic.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

impl fmt::Display for ConfigDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)?;
        if let Some(suggestion) = &self.suggestion {
            write!(formatter, " ({suggestion})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigDiagnostic {}
