//! 应用层拥有的持久化端口。 / Persistence ports owned by the application layer.

use crate::{
    diagnostic::Result,
    domain::{CatalogSnapshot, Metadata, Revision, Symbol, XmlText},
};

/// 一致目录快照的只读端口。 / Read port for a consistent catalog snapshot.
///
/// <!-- @brief 一致目录快照的只读端口。 / Read port for a consistent catalog snapshot. -->
pub trait CatalogRead {
    /// 在一个稳定读事务中装载完整目录。 / Load the complete catalog in one stable read transaction.
    ///
    /// <!-- @brief 在一个稳定读事务中装载完整目录。 / Load the complete catalog in one stable read transaction. -->
    ///
    /// # Returns
    /// 领域快照或结构化诊断。 / Domain snapshot or structured diagnostic.
    ///
    /// <!-- @return 领域快照或结构化诊断。 / Domain snapshot or structured diagnostic. -->
    ///
    /// # Errors
    /// 当适配器无法在一致读事务中装载或验证目录时，返回结构化诊断。 /
    /// Returns a structured diagnostic when the adapter cannot load or validate the catalog in a
    /// consistent read transaction.
    fn snapshot(&self) -> Result<CatalogSnapshot>;
}

/// 写事务内的目录操作端口。 / Catalog operation port inside a write transaction.
///
/// <!-- @brief 写事务内的目录操作端口。 / Catalog operation port inside a write transaction. -->
pub trait CatalogWrite: CatalogRead {
    /// 创建或替换片段。 / Create or replace a fragment.
    ///
    /// <!-- @brief 创建或替换片段。 / Create or replace a fragment. -->
    ///
    /// # Arguments
    /// - `target`: 目标符号。 / Target symbol.
    /// <!-- @param target 目标符号。 / Target symbol. -->
    /// - `text`: 已验证正文。 / Validated body.
    /// <!-- @param text 已验证正文。 / Validated body. -->
    /// - `expected_revision`: 编辑前修订号。 / Revision captured before editing.
    /// <!-- @param expected_revision 编辑前修订号。 / Revision captured before editing. -->
    ///
    /// # Returns
    /// 成功或诊断。 / Success or diagnostic.
    ///
    /// <!-- @return 成功或诊断。 / Success or diagnostic. -->
    ///
    /// # Errors
    /// 当目标状态与预期修订冲突，或适配器无法持久化已验证片段时，返回诊断。 /
    /// Returns a diagnostic when the target state conflicts with the expected revision or the
    /// adapter cannot persist the validated fragment.
    fn upsert_fragment(
        &mut self,
        target: &Symbol,
        text: &XmlText,
        expected_revision: Option<Revision>,
    ) -> Result<()>;

    /// 创建或完整替换提示。 / Create or fully replace a prompt.
    ///
    /// <!-- @brief 创建或完整替换提示。 / Create or fully replace a prompt. -->
    ///
    /// # Arguments
    /// - `target`: 目标符号。 / Target symbol.
    /// <!-- @param target 目标符号。 / Target symbol. -->
    /// - `children`: 有序且可重复的子符号。 / Ordered, duplicate-preserving child symbols.
    /// <!-- @param children 有序且可重复的子符号。 / Ordered, duplicate-preserving child symbols. -->
    ///
    /// # Returns
    /// 成功或诊断。 / Success or diagnostic.
    ///
    /// <!-- @return 成功或诊断。 / Success or diagnostic. -->
    ///
    /// # Errors
    /// 当目标或子节点违反目录约束，或适配器无法持久化提示时，返回诊断。 /
    /// Returns a diagnostic when the target or children violate catalog constraints or the adapter
    /// cannot persist the prompt.
    fn replace_prompt(&mut self, target: &Symbol, children: &[Symbol]) -> Result<()>;

    /// 重命名节点。 / Rename a node.
    ///
    /// <!-- @brief 重命名节点。 / Rename a node. -->
    ///
    /// # Arguments
    /// - `target`: 当前符号。 / Current symbol.
    /// <!-- @param target 当前符号。 / Current symbol. -->
    /// - `new_symbol`: 新符号。 / New symbol.
    /// <!-- @param new_symbol 新符号。 / New symbol. -->
    ///
    /// # Returns
    /// 成功或诊断。 / Success or diagnostic.
    ///
    /// <!-- @return 成功或诊断。 / Success or diagnostic. -->
    ///
    /// # Errors
    /// 当源节点不存在、新符号冲突，或适配器无法持久化重命名时，返回诊断。 /
    /// Returns a diagnostic when the source node is absent, the new symbol conflicts, or the adapter
    /// cannot persist the rename.
    fn rename(&mut self, target: &Symbol, new_symbol: &Symbol) -> Result<()>;

    /// 删除未被引用节点。 / Delete an unreferenced node.
    ///
    /// <!-- @brief 删除未被引用节点。 / Delete an unreferenced node. -->
    ///
    /// # Arguments
    /// - `target`: 目标符号。 / Target symbol.
    /// <!-- @param target 目标符号。 / Target symbol. -->
    ///
    /// # Returns
    /// 成功或诊断。 / Success or diagnostic.
    ///
    /// <!-- @return 成功或诊断。 / Success or diagnostic. -->
    ///
    /// # Errors
    /// 当节点不存在、仍被引用，或适配器无法持久化删除时，返回诊断。 /
    /// Returns a diagnostic when the node is absent, still referenced, or cannot be deleted by the
    /// adapter.
    fn delete(&mut self, target: &Symbol) -> Result<()>;

    /// 完整替换用户元数据。 / Fully replace user metadata.
    ///
    /// <!-- @brief 完整替换用户元数据。 / Fully replace user metadata. -->
    ///
    /// # Arguments
    /// - `target`: 目标符号。 / Target symbol.
    /// <!-- @param target 目标符号。 / Target symbol. -->
    /// - `metadata`: 新元数据。 / New metadata.
    /// <!-- @param metadata 新元数据。 / New metadata. -->
    ///
    /// # Returns
    /// 成功或诊断。 / Success or diagnostic.
    ///
    /// <!-- @return 成功或诊断。 / Success or diagnostic. -->
    ///
    /// # Errors
    /// 当目标不存在或适配器无法持久化元数据时，返回诊断。 /
    /// Returns a diagnostic when the target is absent or the adapter cannot persist the metadata.
    fn set_metadata(&mut self, target: &Symbol, metadata: &Metadata) -> Result<()>;
}

/// 由数据库适配器实现的事务边界。 / Transaction boundary implemented by a database adapter.
///
/// <!-- @brief 由数据库适配器实现的事务边界。 / Transaction boundary implemented by a database adapter. -->
pub trait Database: CatalogRead {
    /// 返回用于检测外部提交的可选变化令牌。 / Return an optional change token for detecting external commits.
    ///
    /// <!-- @brief 返回用于检测外部提交的可选变化令牌。 / Return an optional change token for detecting external commits. -->
    ///
    /// # Returns
    /// 适配器支持时返回连接局部令牌，否则返回 None。 / A connection-local token when supported, otherwise None.
    ///
    /// <!-- @return 适配器支持时返回连接局部令牌，否则返回 None。 / A connection-local token when supported, otherwise None. -->
    ///
    /// # Errors
    /// 当适配器支持变化令牌但无法读取它时，返回诊断。 /
    /// Returns a diagnostic when the adapter supports change tokens but cannot read the token.
    fn change_token(&self) -> Result<Option<u64>> {
        Ok(None)
    }

    /// 在 BEGIN IMMEDIATE 事务中运行闭包并仅在成功时提交。 / Run a closure in BEGIN IMMEDIATE and commit only on success.
    ///
    /// <!-- @brief 在 BEGIN IMMEDIATE 事务中运行闭包并仅在成功时提交。 / Run a closure in BEGIN IMMEDIATE and commit only on success. -->
    ///
    /// # Arguments
    /// - `operation`: 应用层解释闭包。 / Application-layer interpretation closure.
    /// <!-- @param operation 应用层解释闭包。 / Application-layer interpretation closure. -->
    ///
    /// # Returns
    /// 闭包值或结构化诊断。 / Closure value or structured diagnostic.
    ///
    /// <!-- @return 闭包值或结构化诊断。 / Closure value or structured diagnostic. -->
    ///
    /// # Errors
    /// 当事务无法启动或提交，或 `operation` 返回诊断时，返回结构化诊断并回滚事务。 /
    /// Returns a structured diagnostic and rolls back when the transaction cannot begin or commit,
    /// or when `operation` returns a diagnostic.
    fn write_transaction(
        &mut self,
        operation: &mut dyn FnMut(&mut dyn CatalogWrite) -> Result<Vec<super::Value>>,
    ) -> Result<Vec<super::Value>>;
}
