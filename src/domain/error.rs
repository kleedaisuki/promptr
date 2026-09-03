use std::fmt;

use super::NodeId;

/// @brief 领域不变量错误。 / Domain-invariant error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// @brief 数值不是有效的正整数。 / A numeric identity is not positive.
    InvalidPositiveInteger { kind: &'static str, value: i128 },
    /// @brief 符号不符合严格的 XML 名子集。 / A symbol violates the strict XML-name subset.
    InvalidSymbol(String),
    /// @brief 文本包含 XML 1.0 禁止的字符。 / Text contains an XML 1.0-forbidden character.
    InvalidXmlCharacter { character: char, byte_offset: usize },
    /// @brief Prompt 的子节点列表为空。 / A prompt child list is empty.
    EmptyChildren,
    /// @brief 标签为空或仅含空白。 / A tag is empty or whitespace-only.
    EmptyTag,
    /// @brief 节点头类型与节点体类型不一致。 / Node header and body kinds disagree.
    KindMismatch {
        declared: super::NodeKind,
        actual: super::NodeKind,
    },
    /// @brief 快照包含重复的节点标识。 / A snapshot contains a duplicate node id.
    DuplicateNodeId(NodeId),
    /// @brief 快照包含重复的符号。 / A snapshot contains a duplicate symbol.
    DuplicateSymbol(String),
    /// @brief 图引用了不存在的节点。 / The graph references a missing node.
    MissingNode(NodeId),
    /// @brief 图中存在有向环。 / The graph contains a directed cycle.
    CycleDetected { at: NodeId },
    /// @brief 展开路径计数超出 u64。 / Expanded path count exceeds u64.
    OccurrenceOverflow { at: NodeId },
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPositiveInteger { kind, value } => {
                write!(formatter, "{kind} must be positive, got {value}")
            }
            Self::InvalidSymbol(value) => write!(formatter, "invalid symbol `{value}`"),
            Self::InvalidXmlCharacter {
                character,
                byte_offset,
            } => write!(
                formatter,
                "character U+{:04X} at byte {byte_offset} is not valid XML 1.0 text",
                *character as u32
            ),
            Self::EmptyChildren => formatter.write_str("a prompt must contain at least one child"),
            Self::EmptyTag => formatter.write_str("a tag must not be empty or whitespace-only"),
            Self::KindMismatch { declared, actual } => write!(
                formatter,
                "declared node kind {declared:?} does not match body kind {actual:?}"
            ),
            Self::DuplicateNodeId(id) => write!(formatter, "duplicate node id {id}"),
            Self::DuplicateSymbol(symbol) => write!(formatter, "duplicate symbol `{symbol}`"),
            Self::MissingNode(id) => write!(formatter, "missing node {id}"),
            Self::CycleDetected { at } => write!(formatter, "cycle detected at node {at}"),
            Self::OccurrenceOverflow { at } => {
                write!(formatter, "occurrence count overflow at node {at}")
            }
        }
    }
}

impl std::error::Error for DomainError {}
