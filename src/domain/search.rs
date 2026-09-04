use serde::{Deserialize, Serialize};

use super::{NodeId, Symbol};

/// 搜索范围。 / Search scope.
///
/// <!-- @brief 搜索范围。 / Search scope. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchScope {
    /// 搜索整个目录。 / Search the complete catalog.
    ///
    /// <!-- @brief 搜索整个目录。 / Search the complete catalog. -->
    Global,
    /// 仅搜索从指定根节点可达的节点。 / Search only nodes reachable from the given root.
    ///
    /// <!-- @brief 仅搜索从指定根节点可达的节点。 / Search only nodes reachable from the given root. -->
    ReachableFrom(
        /// 可达性查询的根节点。 / Root node for the reachability query.
        ///
        /// <!-- @brief 可达性查询的根节点。 / Root node for the reachability query. -->
        NodeId,
    ),
}

/// 搜索字段维度。 / Search field dimension.
///
/// <!-- @brief 搜索字段维度。 / Search field dimension. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchField {
    /// 仅匹配节点标题（符号）。 / Match node titles (symbols) only.
    ///
    /// <!-- @brief 仅匹配节点标题（符号）。 / Match node titles (symbols) only. -->
    Title,
    /// 仅匹配节点内容。 / Match node content only.
    ///
    /// <!-- @brief 仅匹配节点内容。 / Match node content only. -->
    Content,
    /// 同时匹配标题与内容。 / Match both titles and content.
    ///
    /// <!-- @brief 同时匹配标题与内容。 / Match both titles and content. -->
    Mixed,
}

/// 匹配算法维度。 / Matching algorithm dimension.
///
/// <!-- @brief 匹配算法维度。 / Matching algorithm dimension. -->
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMatcher {
    /// 使用容错的模糊匹配。 / Use typo-tolerant fuzzy matching.
    ///
    /// <!-- @brief 使用容错的模糊匹配。 / Use typo-tolerant fuzzy matching. -->
    Fuzzy,
    /// 使用精确子串匹配。 / Use exact substring matching.
    ///
    /// <!-- @brief 使用精确子串匹配。 / Use exact substring matching. -->
    Exact,
    /// 使用词法全文匹配。 / Use lexical full-text matching.
    ///
    /// <!-- @brief 使用词法全文匹配。 / Use lexical full-text matching. -->
    FullText,
    /// 使用语义匹配。 / Use semantic matching.
    ///
    /// <!-- @brief 使用语义匹配。 / Use semantic matching. -->
    Semantic,
}

/// 与存储实现无关的搜索请求。 / Store-independent search request.
///
/// <!-- @brief 与存储实现无关的搜索请求。 / Store-independent search request. -->
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    /// 用户提供的查询文本。 / User-provided query text.
    ///
    /// <!-- @brief 用户提供的查询文本。 / User-provided query text. -->
    pub text: String,
    /// 搜索图范围。 / Graph scope to search.
    ///
    /// <!-- @brief 搜索图范围。 / Graph scope to search. -->
    pub scope: SearchScope,
    /// 参与匹配的字段。 / Fields participating in matching.
    ///
    /// <!-- @brief 参与匹配的字段。 / Fields participating in matching. -->
    pub field: SearchField,
    /// 使用的匹配算法。 / Matching algorithm to use.
    ///
    /// <!-- @brief 使用的匹配算法。 / Matching algorithm to use. -->
    pub matcher: SearchMatcher,
}

impl SearchQuery {
    /// 构造默认的全局混合模糊搜索。 / Construct the default global mixed fuzzy search.
    ///
    /// # Arguments
    ///
    /// - `text` — 查询文本。 / Query text.
    ///
    /// # Returns
    /// 搜索请求。 / Search request.
    ///
    /// <!-- @brief 构造默认的全局混合模糊搜索。 / Construct the default global mixed fuzzy search. -->
    /// <!-- @param text 查询文本。 / Query text. -->
    /// <!-- @return 搜索请求。 / Search request. -->
    pub fn global(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            scope: SearchScope::Global,
            field: SearchField::Mixed,
            matcher: SearchMatcher::Fuzzy,
        }
    }
}

/// 确定排序所需信息完备的搜索命中。 / Search hit carrying enough data for deterministic ordering.
///
/// <!-- @brief 确定排序所需信息完备的搜索命中。 / Search hit carrying enough data for deterministic ordering. -->
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// 命中节点的稳定标识。 / Stable identity of the matched node.
    ///
    /// <!-- @brief 命中节点的稳定标识。 / Stable identity of the matched node. -->
    pub node_id: NodeId,
    /// 命中节点的符号。 / Symbol of the matched node.
    ///
    /// <!-- @brief 命中节点的符号。 / Symbol of the matched node. -->
    pub symbol: Symbol,
    /// 匹配器产生的相关性分数。 / Relevance score produced by the matcher.
    ///
    /// <!-- @brief 匹配器产生的相关性分数。 / Relevance score produced by the matcher. -->
    pub score: f64,
    /// 节点在展开图中的出现次数。 / Number of occurrences in the expanded graph.
    ///
    /// <!-- @brief 节点在展开图中的出现次数。 / Number of occurrences in the expanded graph. -->
    pub occurrence_count: u64,
}

impl SearchHit {
    /// 按分数降序、符号字节序升序、ID 升序比较。 / Compare by descending score, bytewise symbol, then id.
    ///
    /// # Arguments
    ///
    /// - `other` — 另一命中。 / Other hit.
    ///
    /// # Returns
    /// 确定的排序结果。 / Deterministic ordering.
    ///
    /// <!-- @brief 按分数降序、符号字节序升序、ID 升序比较。 / Compare by descending score, bytewise symbol, then id. -->
    /// <!-- @param other 另一命中。 / Other hit. -->
    /// <!-- @return 确定的排序结果。 / Deterministic ordering. -->
    pub fn deterministic_cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .score
            .total_cmp(&self.score)
            .then_with(|| self.symbol.cmp(&other.symbol))
            .then_with(|| self.node_id.cmp(&other.node_id))
    }
}
