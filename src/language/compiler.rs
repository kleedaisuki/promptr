//! DSL 抽象语法树到领域操作的无副作用语义编译器。 / Pure semantic compiler from DSL AST to domain operations.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    application::{CheckedProgram, Effects, InvocationPolicy, Op, command::NodeFilter},
    diagnostic::{Diagnostic, DiagnosticCategory, RelatedDiagnostic, Result, SourceSpan},
    domain::{CatalogSnapshot, NodeBody, NodeKind, SearchField as DomainSearchField, Symbol, Tag},
};

use super::{
    ListFilter, MetadataValue, Program, SearchField as AstSearchField, Span, Spanned, Statement,
};

/// @brief 编译 DSL 程序并在目录快照之上模拟其顺序效果。 / Compile a DSL program while simulating its ordered effects over a catalog snapshot.
/// @param program 已完成解析的程序。 / Fully parsed program.
/// @param catalog 编译开始时的不可变目录快照。 / Immutable catalog snapshot at compilation start.
/// @param policy 本次调用允许的外部效果。 / External effects allowed for this invocation.
/// @return 已检查程序，或执行任何操作前产生的结构化诊断。 / Checked program, or a structured diagnostic produced before any operation executes.
/// @note 目录覆盖层（catalog overlay）只保存符号、种类与边；编译器绝不修改持久状态。 / The catalog overlay stores only symbols, kinds, and edges; the compiler never mutates persistent state.
pub fn compile(
    program: &Program,
    catalog: &CatalogSnapshot,
    policy: InvocationPolicy,
) -> Result<CheckedProgram> {
    let mut compiler = Compiler::new(catalog)?;
    for statement in &program.statements {
        compiler.compile_statement(statement)?;
    }

    let checked = CheckedProgram::new(compiler.ops);
    validate_capabilities(checked.effects, policy, program.span)?;
    Ok(checked)
}

/// @brief 覆盖层节点的最小语义投影。 / Minimal semantic projection of an overlay node.
#[derive(Clone, Debug)]
struct OverlayNode {
    /// @brief 节点种类。 / Node kind.
    kind: NodeKind,
    /// @brief Prompt 的有序子符号；Fragment 始终为空。 / Ordered child symbols for a Prompt; always empty for a Fragment.
    children: Vec<Symbol>,
}

/// @brief 单次纯编译的可变工作状态。 / Mutable working state for one pure compilation.
struct Compiler {
    /// @brief 按当前可见符号索引的顺序语义覆盖层。 / Ordered semantic overlay indexed by currently visible symbols.
    overlay: BTreeMap<Symbol, OverlayNode>,
    /// @brief 保持源码顺序的已生成操作。 / Generated operations in source order.
    ops: Vec<Op>,
}

impl Compiler {
    /// @brief 从有效快照创建覆盖层。 / Create an overlay from a valid snapshot.
    /// @param catalog 不可变目录快照。 / Immutable catalog snapshot.
    /// @return 编译状态，或快照内部不一致诊断。 / Compiler state, or a snapshot-inconsistency diagnostic.
    fn new(catalog: &CatalogSnapshot) -> Result<Self> {
        let mut overlay = BTreeMap::new();
        for node in catalog.iter_by_symbol() {
            let children = match &node.body {
                NodeBody::Fragment(_) => Vec::new(),
                NodeBody::Prompt(children) => {
                    let mut symbols = Vec::with_capacity(children.as_slice().len());
                    for child in children.as_slice() {
                        let child_node = catalog.get(*child).ok_or_else(|| {
                            Diagnostic::error(
                                "E0199",
                                DiagnosticCategory::Internal,
                                format!("catalog child id `{child}` cannot be resolved"),
                            )
                            .with_hint("reload or repair the catalog before compiling this program")
                        })?;
                        symbols.push(child_node.header.symbol.clone());
                    }
                    symbols
                }
            };
            overlay.insert(
                node.header.symbol.clone(),
                OverlayNode {
                    kind: node.header.kind,
                    children,
                },
            );
        }
        Ok(Self {
            overlay,
            ops: Vec::with_capacity(8),
        })
    }

    /// @brief 编译并模拟一条语句。 / Compile and simulate one statement.
    /// @param statement 带源码位置的语句。 / Spanned statement.
    /// @return 成功或精确语义诊断。 / Success or a precise semantic diagnostic.
    fn compile_statement(&mut self, statement: &Spanned<Statement>) -> Result<()> {
        match &statement.value {
            Statement::Fragment { symbol } => self.compile_fragment(symbol),
            Statement::Prompt { symbol, children } => {
                self.compile_prompt(symbol, children, statement.span)
            }
            Statement::Rename { old, new } => self.compile_rename(old, new),
            Statement::Delete { symbol } => self.compile_delete(symbol),
            Statement::List { filter } => {
                self.ops.push(Op::List {
                    filter: map_list_filter(*filter),
                });
                Ok(())
            }
            Statement::Print { symbol } => {
                let target = self.resolve(symbol, "inspect")?;
                self.ops.push(Op::Inspect { target });
                Ok(())
            }
            Statement::Output { symbol } => {
                let root = self.resolve(symbol, "output")?;
                self.ops.push(Op::RenderXml { root });
                Ok(())
            }
            Statement::Search { query, field } => {
                self.ops.push(Op::Search {
                    query: query.value.clone(),
                    field: field.map(map_search_field),
                    root: None,
                });
                Ok(())
            }
            Statement::Find {
                query,
                prompt,
                field,
            } => {
                let root = self.resolve_kind(prompt, NodeKind::Prompt, "FIND root")?;
                self.ops.push(Op::Search {
                    query: query.value.clone(),
                    field: field.map(map_search_field),
                    root: Some(root),
                });
                Ok(())
            }
            Statement::Metadata { symbol, value } => self.compile_metadata(symbol, value),
        }
    }

    /// @brief 编译 Fragment 创建或编辑。 / Compile Fragment creation or editing.
    /// @param symbol 目标符号。 / Target symbol.
    /// @return 成功或种类诊断。 / Success or a kind diagnostic.
    fn compile_fragment(&mut self, symbol: &Spanned<String>) -> Result<()> {
        let target = checked_symbol(symbol)?;
        if let Some(node) = self.overlay.get(&target) {
            ensure_kind(
                &target,
                node.kind,
                NodeKind::Fragment,
                symbol.span,
                "FRAGMENT",
            )?;
        } else {
            self.overlay.insert(
                target.clone(),
                OverlayNode {
                    kind: NodeKind::Fragment,
                    children: Vec::new(),
                },
            );
        }
        self.ops.push(Op::UpsertFragment {
            target,
            text: None,
            expected_revision: None,
        });
        Ok(())
    }

    /// @brief 编译 Prompt 完整替换并验证所有引用及无环性。 / Compile complete Prompt replacement and validate all references and acyclicity.
    /// @param symbol Prompt 符号。 / Prompt symbol.
    /// @param children 有序子符号。 / Ordered child symbols.
    /// @param statement_span 整条语句的位置。 / Span of the whole statement.
    /// @return 成功或引用、种类、空列表、环诊断。 / Success or a reference, kind, empty-list, or cycle diagnostic.
    fn compile_prompt(
        &mut self,
        symbol: &Spanned<String>,
        children: &[Spanned<String>],
        statement_span: Span,
    ) -> Result<()> {
        let target = checked_symbol(symbol)?;
        if children.is_empty() {
            return Err(error_at(
                "E0107",
                DiagnosticCategory::Domain,
                "a prompt must contain at least one child",
                statement_span,
            )
            .with_hint("add at least one existing node symbol between `[` and `]`"));
        }
        if let Some(node) = self.overlay.get(&target) {
            ensure_kind(
                &target,
                node.kind,
                NodeKind::Prompt,
                symbol.span,
                "prompt declaration",
            )?;
        }

        let mut resolved = Vec::with_capacity(children.len());
        for child in children {
            let candidate = checked_symbol(child)?;
            // A newly declared prompt is visible to its own initializer so the
            // more useful cycle diagnostic wins over an "unknown symbol" error.
            if candidate != target && !self.overlay.contains_key(&candidate) {
                return Err(error_at(
                    "E0101",
                    DiagnosticCategory::ReferentialIntegrity,
                    format!("unknown prompt child symbol `{candidate}`"),
                    child.span,
                )
                .with_hint(
                    "create the node earlier in the program or correct the symbol spelling",
                ));
            }
            resolved.push(candidate);
        }
        self.overlay.insert(
            target.clone(),
            OverlayNode {
                kind: NodeKind::Prompt,
                children: resolved.clone(),
            },
        );
        if let Some(path) = find_cycle(&self.overlay) {
            let printable = path
                .iter()
                .map(Symbol::as_str)
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(error_at(
                "E0105",
                DiagnosticCategory::ReferentialIntegrity,
                format!("prompt declaration introduces cycle: {printable}"),
                statement_span,
            )
            .with_hint("remove an edge in the reported cycle")
            .with_related(RelatedDiagnostic {
                symbol: Some(target.as_str().to_owned()),
                path: Some(path.into_iter().map(Symbol::into_inner).collect()),
                occurrences: None,
                message: "cycle path".to_owned(),
            }));
        }
        self.ops.push(Op::ReplacePrompt {
            target,
            children: resolved,
        });
        Ok(())
    }

    /// @brief 编译重命名并在覆盖层中保留全部拓扑。 / Compile rename while preserving all overlay topology.
    /// @param old 当前符号。 / Current symbol.
    /// @param new 新符号。 / New symbol.
    /// @return 成功或名称解析/冲突诊断。 / Success or name-resolution/conflict diagnostic.
    fn compile_rename(&mut self, old: &Spanned<String>, new: &Spanned<String>) -> Result<()> {
        let target = self.resolve(old, "rename source")?;
        let new_symbol = checked_symbol(new)?;
        if self.overlay.contains_key(&new_symbol) {
            return Err(error_at(
                "E0102",
                DiagnosticCategory::Domain,
                format!("symbol `{new_symbol}` is already bound"),
                new.span,
            )
            .with_hint("choose an unbound destination symbol"));
        }

        let node = self
            .overlay
            .remove(&target)
            .expect("resolved overlay symbol must remain present");
        self.overlay.insert(new_symbol.clone(), node);
        for node in self.overlay.values_mut() {
            for child in &mut node.children {
                if *child == target {
                    *child = new_symbol.clone();
                }
            }
        }
        self.ops.push(Op::Rename { target, new_symbol });
        Ok(())
    }

    /// @brief 编译仅允许无入边节点的删除。 / Compile deletion restricted to nodes without incoming edges.
    /// @param symbol 删除目标。 / Deletion target.
    /// @return 成功或名称/入边诊断。 / Success or a name/incoming-edge diagnostic.
    fn compile_delete(&mut self, symbol: &Spanned<String>) -> Result<()> {
        let target = self.resolve(symbol, "delete target")?;
        let mut parents = Vec::new();
        for (parent, node) in &self.overlay {
            let occurrences = node
                .children
                .iter()
                .filter(|child| **child == target)
                .count();
            if occurrences != 0 {
                parents.push(RelatedDiagnostic {
                    symbol: Some(parent.as_str().to_owned()),
                    path: None,
                    occurrences: Some(occurrences as u64),
                    message: "referring prompt".to_owned(),
                });
            }
        }
        if !parents.is_empty() {
            let mut diagnostic = error_at(
                "E0104",
                DiagnosticCategory::ReferentialIntegrity,
                format!("symbol `{target}` is still referenced"),
                symbol.span,
            )
            .with_hint("replace or delete every referring prompt before deleting this node");
            diagnostic.related = parents;
            return Err(diagnostic);
        }
        self.overlay.remove(&target);
        self.ops.push(Op::Delete { target });
        Ok(())
    }

    /// @brief 编译元数据字段的完整替换。 / Compile complete replacement of a metadata field.
    /// @param symbol 现有目标符号。 / Existing target symbol.
    /// @param value 元数据替换值。 / Metadata replacement value.
    /// @return 成功或目标/标签诊断。 / Success or a target/tag diagnostic.
    fn compile_metadata(&mut self, symbol: &Spanned<String>, value: &MetadataValue) -> Result<()> {
        let target = self.resolve(symbol, "metadata target")?;
        match value {
            MetadataValue::Description(description) => {
                self.ops.push(Op::SetDescription {
                    target,
                    description: (!description.value.is_empty()).then(|| description.value.clone()),
                });
            }
            MetadataValue::Tags(tags) => {
                let mut normalized = BTreeSet::new();
                for tag in tags {
                    let checked = Tag::new(tag.value.clone()).map_err(|_| {
                        error_at(
                            "E0108",
                            DiagnosticCategory::Domain,
                            "metadata tags cannot be empty or whitespace-only",
                            tag.span,
                        )
                        .with_hint("remove the empty tag or give it a non-whitespace name")
                    })?;
                    normalized.insert(checked.into_inner());
                }
                self.ops.push(Op::SetTags {
                    target,
                    tags: normalized.into_iter().collect(),
                });
            }
        }
        Ok(())
    }

    /// @brief 解析当前覆盖层中的现有符号。 / Resolve an existing symbol in the current overlay.
    /// @param symbol 带位置的候选符号。 / Spanned candidate symbol.
    /// @param role 符号在语句中的角色。 / Symbol role within the statement.
    /// @return 领域符号或名称解析诊断。 / Domain symbol or a name-resolution diagnostic.
    fn resolve(&self, symbol: &Spanned<String>, role: &str) -> Result<Symbol> {
        let checked = checked_symbol(symbol)?;
        if self.overlay.contains_key(&checked) {
            return Ok(checked);
        }
        Err(error_at(
            "E0101",
            DiagnosticCategory::ReferentialIntegrity,
            format!("unknown {role} symbol `{checked}`"),
            symbol.span,
        )
        .with_hint("create the node earlier in the program or correct the symbol spelling"))
    }

    /// @brief 解析现有符号并检查节点种类。 / Resolve an existing symbol and check its node kind.
    /// @param symbol 带位置的候选符号。 / Spanned candidate symbol.
    /// @param expected 所需节点种类。 / Required node kind.
    /// @param role 符号在语句中的角色。 / Symbol role within the statement.
    /// @return 领域符号或解析/种类诊断。 / Domain symbol or a resolution/kind diagnostic.
    fn resolve_kind(
        &self,
        symbol: &Spanned<String>,
        expected: NodeKind,
        role: &str,
    ) -> Result<Symbol> {
        let checked = self.resolve(symbol, role)?;
        let actual = self.overlay[&checked].kind;
        ensure_kind(&checked, actual, expected, symbol.span, role)?;
        Ok(checked)
    }
}

/// @brief 为结构化诊断附加相关实体的局部扩展。 / Local extension for attaching a related entity to a structured diagnostic.
trait DiagnosticRelatedExt {
    /// @brief 附加相关实体。 / Attach a related entity.
    /// @param related 相关实体。 / Related entity.
    /// @return 更新后的诊断。 / Updated diagnostic.
    fn with_related(self, related: RelatedDiagnostic) -> Self;
}

impl DiagnosticRelatedExt for Diagnostic {
    fn with_related(mut self, related: RelatedDiagnostic) -> Self {
        self.related.push(related);
        self
    }
}

/// @brief 验证并转换 AST 符号。 / Validate and convert an AST symbol.
/// @param symbol 带源码位置的符号文本。 / Spanned symbol text.
/// @return 领域符号或稳定诊断。 / Domain symbol or a stable diagnostic.
fn checked_symbol(symbol: &Spanned<String>) -> Result<Symbol> {
    Symbol::new(symbol.value.clone()).map_err(|_| {
        error_at(
            "E0106",
            DiagnosticCategory::Domain,
            format!("invalid symbol `{}`", symbol.value),
            symbol.span,
        )
        .with_hint("use `[A-Za-z_][A-Za-z0-9_-]*`")
    })
}

/// @brief 检查节点种类与命令要求相符。 / Check that a node kind matches a command requirement.
/// @param symbol 被检查符号。 / Checked symbol.
/// @param actual 实际种类。 / Actual kind.
/// @param expected 期望种类。 / Expected kind.
/// @param span 符号位置。 / Symbol span.
/// @param role 命令角色。 / Command role.
/// @return 匹配时成功，否则返回稳定诊断。 / Success on match, otherwise a stable diagnostic.
fn ensure_kind(
    symbol: &Symbol,
    actual: NodeKind,
    expected: NodeKind,
    span: Span,
    role: &str,
) -> Result<()> {
    if actual == expected {
        return Ok(());
    }
    Err(error_at(
        "E0103",
        DiagnosticCategory::Domain,
        format!(
            "{role} requires a {}, but `{symbol}` is a {}",
            kind_name(expected),
            kind_name(actual)
        ),
        span,
    )
    .with_hint(format!(
        "choose an existing {} symbol or use a command valid for {} nodes",
        kind_name(expected),
        kind_name(actual)
    )))
}

/// @brief 返回稳定的人类可读节点种类名。 / Return a stable human-readable node-kind name.
/// @param kind 节点种类。 / Node kind.
/// @return 小写英文名称。 / Lowercase English name.
const fn kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Fragment => "fragment",
        NodeKind::Prompt => "prompt",
    }
}

/// @brief 使用迭代深度优先搜索查找一个确定性的环路径。 / Find one deterministic cycle path using iterative depth-first search.
/// @param overlay 当前目录覆盖层。 / Current catalog overlay.
/// @return 首尾重复的环路径，无环时为空。 / Cycle path with repeated first/last symbol, or none when acyclic.
fn find_cycle(overlay: &BTreeMap<Symbol, OverlayNode>) -> Option<Vec<Symbol>> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let mut colors: BTreeMap<Symbol, Color> = overlay
        .keys()
        .cloned()
        .map(|symbol| (symbol, Color::White))
        .collect();
    for start in overlay.keys() {
        if colors[start] != Color::White {
            continue;
        }
        colors.insert(start.clone(), Color::Gray);
        let mut stack = vec![(start.clone(), 0usize)];
        while let Some((symbol, next_child)) = stack.last_mut() {
            let children = &overlay[symbol].children;
            if *next_child == children.len() {
                colors.insert(symbol.clone(), Color::Black);
                stack.pop();
                continue;
            }
            let child = children[*next_child].clone();
            *next_child += 1;
            match colors.get(&child).copied().unwrap_or(Color::White) {
                Color::Black => {}
                Color::White => {
                    colors.insert(child.clone(), Color::Gray);
                    stack.push((child, 0));
                }
                Color::Gray => {
                    let cycle_start = stack
                        .iter()
                        .position(|(ancestor, _)| *ancestor == child)
                        .unwrap_or(0);
                    let mut path = stack[cycle_start..]
                        .iter()
                        .map(|(ancestor, _)| ancestor.clone())
                        .collect::<Vec<_>>();
                    path.push(child);
                    return Some(path);
                }
            }
        }
    }
    None
}

/// @brief 在计划执行前一次性验证聚合效果。 / Validate aggregate effects once before planned execution.
/// @param required 程序所需效果的并集。 / Union of effects required by the program.
/// @param policy 调用策略。 / Invocation policy.
/// @param span 整个程序的位置。 / Whole-program span.
/// @return 能力足够时成功，否则返回缺失能力诊断。 / Success when capabilities suffice, otherwise a missing-capability diagnostic.
fn validate_capabilities(required: Effects, policy: InvocationPolicy, span: Span) -> Result<()> {
    let missing = required - policy.allowed_effects;
    if missing.is_empty() {
        return Ok(());
    }
    let names = effect_names(missing);
    Err(error_at(
        "E0201",
        DiagnosticCategory::Capability,
        format!(
            "invocation policy does not allow required effects: {}",
            names.join(", ")
        ),
        span,
    )
    .with_hint("use an invocation mode or policy that explicitly permits every listed effect"))
}

/// @brief 将效果位转换为确定顺序的名称。 / Convert effect bits to deterministically ordered names.
/// @param effects 效果集合。 / Effect set.
/// @return 稳定顺序名称。 / Names in stable order.
fn effect_names(effects: Effects) -> Vec<&'static str> {
    [
        (Effects::READ_STORE, "read_store"),
        (Effects::WRITE_STORE, "write_store"),
        (Effects::READ_FILE, "read_file"),
        (Effects::READ_STDIN, "read_stdin"),
        (Effects::INTERACTIVE, "interactive"),
    ]
    .into_iter()
    .filter_map(|(effect, name)| effects.contains(effect).then_some(name))
    .collect()
}

/// @brief 映射列表过滤器到应用 IR。 / Map a list filter to application IR.
/// @param filter AST 过滤器。 / AST filter.
/// @return IR 过滤器。 / IR filter.
const fn map_list_filter(filter: ListFilter) -> NodeFilter {
    match filter {
        ListFilter::All => NodeFilter::All,
        ListFilter::Fragments => NodeFilter::Fragments,
        ListFilter::Prompts => NodeFilter::Prompts,
    }
}

/// @brief 映射搜索字段到领域枚举。 / Map a search field to the domain enum.
/// @param field AST 搜索字段。 / AST search field.
/// @return 领域搜索字段。 / Domain search field.
const fn map_search_field(field: AstSearchField) -> DomainSearchField {
    match field {
        AstSearchField::Title => DomainSearchField::Title,
        AstSearchField::Content => DomainSearchField::Content,
        AstSearchField::Mixed => DomainSearchField::Mixed,
    }
}

/// @brief 构造带源码位置的稳定错误诊断。 / Construct a stable error diagnostic with source span.
/// @param code 稳定错误码。 / Stable error code.
/// @param category 诊断分类。 / Diagnostic category.
/// @param message 用户可读消息。 / User-readable message.
/// @param span AST 字节区间。 / AST byte range.
/// @return 结构化诊断。 / Structured diagnostic.
fn error_at(
    code: &'static str,
    category: DiagnosticCategory,
    message: impl Into<String>,
    span: Span,
) -> Diagnostic {
    Diagnostic::error(code, category, message).with_span(SourceSpan::new(span.start, span.end))
}

#[cfg(test)]
mod tests {
    use crate::domain::{Metadata, Node, NodeHeader, NodeId, NonEmptyChildren, Revision, XmlText};

    use super::*;

    fn spanned(value: impl Into<String>) -> Spanned<String> {
        Spanned::new(value.into(), Span::new(1, 2))
    }

    fn program(statements: Vec<Statement>) -> Program {
        Program {
            statements: statements
                .into_iter()
                .map(|statement| Spanned::new(statement, Span::new(0, 10)))
                .collect(),
            span: Span::new(0, 10),
        }
    }

    fn id(value: i64) -> NodeId {
        NodeId::new(value).unwrap()
    }

    fn node(value: i64, symbol: &str, body: NodeBody) -> Node {
        Node::from_parts(
            NodeHeader {
                id: id(value),
                symbol: Symbol::new(symbol).unwrap(),
                kind: body.kind(),
                revision: Revision::new(1).unwrap(),
                metadata: Metadata::default(),
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            body,
        )
        .unwrap()
    }

    fn fragment(value: i64, symbol: &str) -> Node {
        node(value, symbol, NodeBody::Fragment(XmlText::new("").unwrap()))
    }

    fn prompt(value: i64, symbol: &str, children: Vec<NodeId>) -> Node {
        node(
            value,
            symbol,
            NodeBody::Prompt(NonEmptyChildren::new(children).unwrap()),
        )
    }

    #[test]
    fn supports_create_then_reference_and_declares_interaction() {
        let source = program(vec![
            Statement::Fragment {
                symbol: spanned("Leaf"),
            },
            Statement::Prompt {
                symbol: spanned("Root"),
                children: vec![spanned("Leaf")],
            },
        ]);
        let checked = compile(
            &source,
            &CatalogSnapshot::default(),
            InvocationPolicy::interactive(),
        )
        .unwrap();
        assert_eq!(checked.ops.len(), 2);
        assert!(checked.effects.contains(Effects::INTERACTIVE));
        assert!(matches!(
            &checked.ops[0],
            Op::UpsertFragment { text: None, .. }
        ));
    }

    #[test]
    fn rename_preserves_incoming_edges_for_later_delete_checks() {
        let catalog =
            CatalogSnapshot::new([fragment(1, "Leaf"), prompt(2, "Root", vec![id(1)])]).unwrap();
        let source = program(vec![
            Statement::Rename {
                old: spanned("Leaf"),
                new: spanned("Renamed"),
            },
            Statement::Delete {
                symbol: spanned("Renamed"),
            },
        ]);
        let error = compile(&source, &catalog, InvocationPolicy::interactive()).unwrap_err();
        assert_eq!(error.code, "E0104");
        assert_eq!(error.related[0].symbol.as_deref(), Some("Root"));
    }

    #[test]
    fn supports_rename_then_reference() {
        let catalog = CatalogSnapshot::new([fragment(1, "Leaf")]).unwrap();
        let source = program(vec![
            Statement::Rename {
                old: spanned("Leaf"),
                new: spanned("Renamed"),
            },
            Statement::Prompt {
                symbol: spanned("Root"),
                children: vec![spanned("Renamed")],
            },
        ]);
        assert!(compile(&source, &catalog, InvocationPolicy::interactive()).is_ok());
    }

    #[test]
    fn detects_cycle_formed_across_statements_with_path() {
        let catalog = CatalogSnapshot::new([fragment(1, "Leaf")]).unwrap();
        let source = program(vec![
            Statement::Prompt {
                symbol: spanned("A"),
                children: vec![spanned("Leaf")],
            },
            Statement::Prompt {
                symbol: spanned("B"),
                children: vec![spanned("A")],
            },
            Statement::Prompt {
                symbol: spanned("A"),
                children: vec![spanned("B")],
            },
        ]);
        let error = compile(&source, &catalog, InvocationPolicy::interactive()).unwrap_err();
        assert_eq!(error.code, "E0105");
        assert_eq!(
            error.related[0].path.as_deref(),
            Some(&["A".to_owned(), "B".to_owned(), "A".to_owned()][..])
        );
    }

    #[test]
    fn direct_self_reference_is_reported_as_a_cycle() {
        let source = program(vec![Statement::Prompt {
            symbol: spanned("Loop"),
            children: vec![spanned("Loop")],
        }]);
        let error = compile(
            &source,
            &CatalogSnapshot::default(),
            InvocationPolicy::interactive(),
        )
        .unwrap_err();
        assert_eq!(error.code, "E0105");
        assert_eq!(
            error.related[0].path.as_deref(),
            Some(&["Loop".to_owned(), "Loop".to_owned()][..])
        );
    }

    #[test]
    fn replacing_parent_edges_allows_a_later_delete() {
        let catalog = CatalogSnapshot::new([
            fragment(1, "Old"),
            fragment(2, "Kept"),
            prompt(3, "Root", vec![id(1)]),
        ])
        .unwrap();
        let source = program(vec![
            Statement::Prompt {
                symbol: spanned("Root"),
                children: vec![spanned("Kept")],
            },
            Statement::Delete {
                symbol: spanned("Old"),
            },
        ]);
        assert!(compile(&source, &catalog, InvocationPolicy::interactive()).is_ok());
    }

    #[test]
    fn tag_sets_are_validated_sorted_and_deduplicated() {
        let catalog = CatalogSnapshot::new([fragment(1, "Leaf")]).unwrap();
        let source = program(vec![Statement::Metadata {
            symbol: spanned("Leaf"),
            value: MetadataValue::Tags(vec![spanned("z"), spanned("a"), spanned("z")]),
        }]);
        let checked = compile(&source, &catalog, InvocationPolicy::interactive()).unwrap();
        assert!(matches!(
            &checked.ops[0],
            Op::SetTags { tags, .. } if tags == &["a".to_owned(), "z".to_owned()]
        ));

        let invalid = program(vec![Statement::Metadata {
            symbol: spanned("Leaf"),
            value: MetadataValue::Tags(vec![spanned("  \t")]),
        }]);
        assert_eq!(
            compile(&invalid, &catalog, InvocationPolicy::interactive())
                .unwrap_err()
                .code,
            "E0108"
        );
    }

    #[test]
    fn capability_check_reports_aggregate_missing_effects() {
        let source = program(vec![Statement::Fragment {
            symbol: spanned("Leaf"),
        }]);
        let error = compile(
            &source,
            &CatalogSnapshot::default(),
            InvocationPolicy::script(),
        )
        .unwrap_err();
        assert_eq!(error.code, "E0201");
        assert!(error.message.contains("interactive"));
    }

    #[test]
    fn find_requires_prompt_but_output_accepts_fragments() {
        let catalog = CatalogSnapshot::new([fragment(1, "Leaf")]).unwrap();
        let output = program(vec![Statement::Output {
            symbol: spanned("Leaf"),
        }]);
        assert!(compile(&output, &catalog, InvocationPolicy::interactive()).is_ok());

        let find = program(vec![Statement::Find {
            query: spanned("x"),
            prompt: spanned("Leaf"),
            field: Some(AstSearchField::Mixed),
        }]);
        assert_eq!(
            compile(&find, &catalog, InvocationPolicy::interactive())
                .unwrap_err()
                .code,
            "E0103"
        );
    }

    #[test]
    fn search_ir_preserves_an_omitted_field() {
        let source = program(vec![Statement::Search {
            query: spanned("needle"),
            field: None,
        }]);
        let checked = compile(
            &source,
            &CatalogSnapshot::default(),
            InvocationPolicy::script(),
        )
        .unwrap();
        assert!(matches!(&checked.ops[0], Op::Search { field: None, .. }));
    }
}
