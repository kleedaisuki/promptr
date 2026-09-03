//! @brief Promptr 的纯领域模型。 / Pure domain model for Promptr.

mod catalog;
mod error;
mod metadata;
mod node;
mod search;

pub use catalog::{CatalogSnapshot, RenderError};
pub use error::DomainError;
pub use metadata::{Metadata, Tag};
pub use node::{
    Node, NodeBody, NodeHeader, NodeId, NodeKind, NonEmptyChildren, Revision, Symbol, XmlText,
};
/// @brief 搜索字段的简洁领域别名。 / Concise domain alias for a search field.
pub use search::SearchField as Field;
/// @brief 搜索匹配器的简洁领域别名。 / Concise domain alias for a search matcher.
pub use search::SearchMatcher as Matcher;
/// @brief 搜索范围的简洁领域别名。 / Concise domain alias for a search scope.
pub use search::SearchScope as Scope;
pub use search::{SearchField, SearchHit, SearchMatcher, SearchQuery, SearchScope};
