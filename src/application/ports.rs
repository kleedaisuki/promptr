//! 应用层拥有的持久化端口。 / Persistence ports owned by the application layer.

use crate::{
    diagnostic::Result,
    domain::{CatalogSnapshot, Metadata, Revision, Symbol, XmlText},
};

/// @brief 一致目录快照的只读端口。 / Read port for a consistent catalog snapshot.
pub trait CatalogRead {
    /// @brief 在一个稳定读事务中装载完整目录。 / Load the complete catalog in one stable read transaction.
    /// @return 领域快照或结构化诊断。 / Domain snapshot or structured diagnostic.
    fn snapshot(&self) -> Result<CatalogSnapshot>;
}

/// @brief 写事务内的目录操作端口。 / Catalog operation port inside a write transaction.
pub trait CatalogWrite: CatalogRead {
    /// @brief 创建或替换片段。 / Create or replace a fragment.
    /// @param target 目标符号。 / Target symbol.
    /// @param text 已验证正文。 / Validated body.
    /// @param expected_revision 编辑前修订号。 / Revision captured before editing.
    /// @return 成功或诊断。 / Success or diagnostic.
    fn upsert_fragment(
        &mut self,
        target: &Symbol,
        text: &XmlText,
        expected_revision: Option<Revision>,
    ) -> Result<()>;

    /// @brief 创建或完整替换提示。 / Create or fully replace a prompt.
    /// @param target 目标符号。 / Target symbol.
    /// @param children 有序且可重复的子符号。 / Ordered, duplicate-preserving child symbols.
    /// @return 成功或诊断。 / Success or diagnostic.
    fn replace_prompt(&mut self, target: &Symbol, children: &[Symbol]) -> Result<()>;

    /// @brief 重命名节点。 / Rename a node.
    /// @param target 当前符号。 / Current symbol.
    /// @param new_symbol 新符号。 / New symbol.
    /// @return 成功或诊断。 / Success or diagnostic.
    fn rename(&mut self, target: &Symbol, new_symbol: &Symbol) -> Result<()>;

    /// @brief 删除未被引用节点。 / Delete an unreferenced node.
    /// @param target 目标符号。 / Target symbol.
    /// @return 成功或诊断。 / Success or diagnostic.
    fn delete(&mut self, target: &Symbol) -> Result<()>;

    /// @brief 完整替换用户元数据。 / Fully replace user metadata.
    /// @param target 目标符号。 / Target symbol.
    /// @param metadata 新元数据。 / New metadata.
    /// @return 成功或诊断。 / Success or diagnostic.
    fn set_metadata(&mut self, target: &Symbol, metadata: &Metadata) -> Result<()>;
}

/// @brief 由数据库适配器实现的事务边界。 / Transaction boundary implemented by a database adapter.
pub trait Database: CatalogRead {
    /// @brief 返回用于检测外部提交的可选变化令牌。 / Return an optional change token for detecting external commits.
    /// @return 适配器支持时返回连接局部令牌，否则返回 None。 / A connection-local token when supported, otherwise None.
    fn change_token(&self) -> Result<Option<u64>> {
        Ok(None)
    }

    /// @brief 在 BEGIN IMMEDIATE 事务中运行闭包并仅在成功时提交。 / Run a closure in BEGIN IMMEDIATE and commit only on success.
    /// @param operation 应用层解释闭包。 / Application-layer interpretation closure.
    /// @return 闭包值或结构化诊断。 / Closure value or structured diagnostic.
    fn write_transaction(
        &mut self,
        operation: &mut dyn FnMut(&mut dyn CatalogWrite) -> Result<Vec<super::Value>>,
    ) -> Result<Vec<super::Value>>;
}
