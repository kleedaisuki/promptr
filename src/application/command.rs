//! 编译后领域操作。 / Compiled domain-shaped operations.

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

use crate::domain::{Revision, SearchField, Symbol, XmlText};

bitflags! {
    /// @brief 调用在执行前声明的外部效果集合。 / External effect set declared before execution.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
    pub struct Effects: u8 {
        /// @brief 读取持久目录。 / Read the persistent catalog.
        const READ_STORE  = 1 << 0;
        /// @brief 修改持久目录。 / Mutate the persistent catalog.
        const WRITE_STORE = 1 << 1;
        /// @brief 读取普通文件。 / Read a regular file.
        const READ_FILE   = 1 << 2;
        /// @brief 读取标准输入。 / Read standard input.
        const READ_STDIN  = 1 << 3;
        /// @brief 请求交互输入。 / Request interactive input.
        const INTERACTIVE = 1 << 4;
    }
}

/// @brief 列表过滤器。 / Catalog list filter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum NodeFilter {
    /// @brief 所有节点。 / All nodes.
    #[default]
    All,
    /// @brief 仅片段。 / Fragments only.
    Fragments,
    /// @brief 仅提示。 / Prompts only.
    Prompts,
}

/// @brief 已验证、领域形的中间表示操作。 / Checked, domain-shaped IR operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Op {
    /// @brief 创建或替换片段；未准备时 text 为 None。 / Create or replace a fragment; text is None before preparation.
    UpsertFragment {
        /// @brief 目标符号。 / Target symbol.
        target: Symbol,
        /// @brief 已准备并验证的正文。 / Prepared and validated body.
        text: Option<XmlText>,
        /// @brief 长时编辑前的修订号。 / Revision captured before a long-lived edit.
        expected_revision: Option<Revision>,
    },
    /// @brief 创建或完整替换提示的子节点列表。 / Create or fully replace a prompt child list.
    ReplacePrompt {
        /// @brief 目标符号。 / Target symbol.
        target: Symbol,
        /// @brief 有序且可重复的子符号。 / Ordered, duplicate-preserving child symbols.
        children: Vec<Symbol>,
    },
    /// @brief 重命名并保留稳定 NodeId。 / Rename while preserving stable NodeId.
    Rename {
        /// @brief 现有符号。 / Existing symbol.
        target: Symbol,
        /// @brief 新符号。 / New symbol.
        new_symbol: Symbol,
    },
    /// @brief 删除未被引用的节点。 / Delete an unreferenced node.
    Delete {
        /// @brief 目标符号。 / Target symbol.
        target: Symbol,
    },
    /// @brief 完整替换描述。 / Fully replace a description.
    SetDescription {
        /// @brief 目标符号。 / Target symbol.
        target: Symbol,
        /// @brief 空值表示清除。 / None means clear.
        description: Option<String>,
    },
    /// @brief 完整替换标签集合。 / Fully replace the tag set.
    SetTags {
        /// @brief 目标符号。 / Target symbol.
        target: Symbol,
        /// @brief 已验证且确定排序的标签。 / Validated, deterministically ordered tags.
        tags: Vec<String>,
    },
    /// @brief 列出节点。 / List nodes.
    List {
        /// @brief 节点过滤器。 / Node filter.
        filter: NodeFilter,
    },
    /// @brief 人类可读检查。 / Human-oriented inspection.
    Inspect {
        /// @brief 目标符号。 / Target symbol.
        target: Symbol,
    },
    /// @brief 搜索全局或可达片段。 / Search global or reachable fragments.
    Search {
        /// @brief 查询文本。 / Query text.
        query: String,
        /// @brief 显式搜索字段；None 表示使用运行时配置。 / Explicit search field; None uses runtime configuration.
        field: Option<SearchField>,
        /// @brief FIND 的可选提示根。 / Optional prompt root for FIND.
        root: Option<Symbol>,
    },
    /// @brief 生成规范 XML。 / Render canonical XML.
    RenderXml {
        /// @brief 根符号。 / Root symbol.
        root: Symbol,
    },
}

impl Op {
    /// @brief 返回单个操作声明的效果。 / Return effects declared by one operation.
    /// @return 效果集合。 / Effect set.
    #[must_use]
    pub const fn effects(&self) -> Effects {
        match self {
            Self::UpsertFragment { .. } => Effects::READ_STORE
                .union(Effects::WRITE_STORE)
                .union(Effects::INTERACTIVE),
            Self::ReplacePrompt { .. }
            | Self::Rename { .. }
            | Self::Delete { .. }
            | Self::SetDescription { .. }
            | Self::SetTags { .. } => Effects::READ_STORE.union(Effects::WRITE_STORE),
            Self::List { .. }
            | Self::Inspect { .. }
            | Self::Search { .. }
            | Self::RenderXml { .. } => Effects::READ_STORE,
        }
    }
}

/// @brief 编译且通过语义检查的程序。 / Compiled and semantically checked program.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CheckedProgram {
    /// @brief 保持源顺序的操作。 / Operations in source order.
    pub ops: Vec<Op>,
    /// @brief 所有操作效果的并集。 / Union of effects for all operations.
    pub effects: Effects,
}

impl CheckedProgram {
    /// @brief 从操作构造程序并自动聚合效果。 / Build a program and aggregate effects.
    /// @param ops 已检查操作。 / Checked operations.
    /// @return 已检查程序。 / Checked program.
    #[must_use]
    pub fn new(ops: Vec<Op>) -> Self {
        let effects = ops
            .iter()
            .fold(Effects::empty(), |effects, op| effects | op.effects());
        Self { ops, effects }
    }

    /// @brief 判断程序是否会修改目录。 / Determine whether the program mutates the catalog.
    /// @return 若包含写效果则为真。 / True when a write effect is present.
    #[must_use]
    pub fn is_mutating(&self) -> bool {
        self.effects.contains(Effects::WRITE_STORE)
    }
}
