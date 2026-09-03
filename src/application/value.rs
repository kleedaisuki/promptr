//! 解释器返回的宿主无关值。 / Host-independent values returned by the interpreter.

use serde::{Deserialize, Serialize};

use crate::domain::{Metadata, NodeId, NodeKind, Revision, SearchHit, Symbol};

/// @brief 面向宿主的节点投影。 / Node projection exposed to hosts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeView {
    /// @brief 稳定内部标识。 / Stable internal identity.
    pub id: NodeId,
    /// @brief 可见符号。 / Visible symbol.
    pub symbol: Symbol,
    /// @brief 节点种类。 / Node kind.
    pub kind: NodeKind,
    /// @brief 修订号。 / Revision.
    pub revision: Revision,
    /// @brief 有序直接子符号。 / Ordered direct child symbols.
    pub children: Vec<Symbol>,
    /// @brief 片段字节数。 / Fragment byte length.
    pub byte_size: Option<usize>,
    /// @brief 用户元数据。 / User metadata.
    pub metadata: Metadata,
}

/// @brief 搜索命中的稳定接口投影。 / Stable interface projection of a search hit.
pub type SearchHitView = SearchHit;

/// @brief 解释器产生的类型化值。 / Typed value produced by the interpreter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    /// @brief 无负载的成功。 / Successful operation without a payload.
    Unit,
    /// @brief 人类向文本。 / Human-oriented text.
    Text(String),
    /// @brief 规范 XML 字节的 UTF-8 表示。 / UTF-8 representation of canonical XML bytes.
    Xml(String),
    /// @brief 单节点投影。 / One node projection.
    Node(NodeView),
    /// @brief 节点列表。 / Node list.
    Nodes(Vec<NodeView>),
    /// @brief 搜索结果。 / Search results.
    SearchResults(Vec<SearchHitView>),
    /// @brief 用户元数据。 / User metadata.
    Metadata(Metadata),
}
