use serde::{Deserialize, Serialize};

use super::{NodeId, Symbol};

/// @brief 搜索范围。 / Search scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchScope {
    Global,
    ReachableFrom(NodeId),
}

/// @brief 搜索字段维度。 / Search field dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchField {
    Title,
    Content,
    Mixed,
}

/// @brief 匹配算法维度。 / Matching algorithm dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMatcher {
    Fuzzy,
    Exact,
    FullText,
    Semantic,
}

/// @brief 与存储实现无关的搜索请求。 / Store-independent search request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub scope: SearchScope,
    pub field: SearchField,
    pub matcher: SearchMatcher,
}

impl SearchQuery {
    /// @brief 构造默认的全局混合模糊搜索。 / Construct the default global mixed fuzzy search.
    /// @param text 查询文本。 / Query text.
    /// @return 搜索请求。 / Search request.
    pub fn global(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            scope: SearchScope::Global,
            field: SearchField::Mixed,
            matcher: SearchMatcher::Fuzzy,
        }
    }
}

/// @brief 确定排序所需信息完备的搜索命中。 / Search hit carrying enough data for deterministic ordering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub node_id: NodeId,
    pub symbol: Symbol,
    pub score: f64,
    pub occurrence_count: u64,
}

impl SearchHit {
    /// @brief 按分数降序、符号字节序升序、ID 升序比较。 / Compare by descending score, bytewise symbol, then id.
    /// @param other 另一命中。 / Other hit.
    /// @return 确定的排序结果。 / Deterministic ordering.
    pub fn deterministic_cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .score
            .total_cmp(&self.score)
            .then_with(|| self.symbol.cmp(&other.symbol))
            .then_with(|| self.node_id.cmp(&other.node_id))
    }
}
