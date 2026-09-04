//! 编译、准备、事务和解释的协调器。 / Coordinator for compile, prepare, transaction, and interpretation.

use super::{InvocationPolicy, NodePrecondition, Promptr, SpillWriter, Value, ports::CatalogWrite};
use crate::{
    application::{CheckedProgram, NodeView, Op, command::NodeFilter},
    diagnostic::{Diagnostic, DiagnosticCategory, Result, SourceSpan},
    domain::{
        CatalogSnapshot, DomainError, Metadata, Node, NodeBody, NodeKind, SearchField, SearchHit,
        Symbol, Tag, XmlText,
    },
    infrastructure::editor::{EditRequest, EditTarget, EditorOutcome, TextProvider},
    language::{ParseOutcome, Program, parse},
};
use nucleo_matcher::{Config as MatcherConfig, Matcher, Utf32Str};
use std::collections::BTreeMap;

/// 单次求值固定的搜索配置。 / Search configuration fixed for one evaluation.
///
/// <!-- @brief 单次求值固定的搜索配置。 / Search configuration fixed for one evaluation. -->
#[derive(Clone, Copy)]
struct SearchSettings {
    /// DSL 省略 FROM 时采用的字段。 / Field used when DSL omits FROM.
    ///
    /// <!-- @brief DSL 省略 FROM 时采用的字段。 / Field used when DSL omits FROM. -->
    default_field: SearchField,
    /// 本次求值采用的匹配算法。 / Matching algorithm used by this evaluation.
    ///
    /// <!-- @brief 本次求值采用的匹配算法。 / Matching algorithm used by this evaluation. -->
    matcher: SearchMatcher,
}

/// 运行时支持的搜索匹配算法。 / Search matching algorithms supported by the runtime.
///
/// <!-- @brief 运行时支持的搜索匹配算法。 / Search matching algorithms supported by the runtime. -->
#[derive(Clone, Copy)]
enum SearchMatcher {
    /// Nucleo 模糊匹配。 / Nucleo fuzzy matching.
    ///
    /// <!-- @brief Nucleo 模糊匹配。 / Nucleo fuzzy matching. -->
    Fuzzy,
    /// 大小写敏感的子串匹配。 / Case-sensitive substring matching.
    ///
    /// <!-- @brief 大小写敏感的子串匹配。 / Case-sensitive substring matching. -->
    Exact,
}

impl SearchSettings {
    /// 从已验证配置构造运行时设置。 / Build runtime settings from validated configuration.
    ///
    /// <!-- @brief 从已验证配置构造运行时设置。 / Build runtime settings from validated configuration. -->
    ///
    /// # Arguments
    /// - `config`: 已验证搜索配置。 / Validated search configuration.
    /// <!-- @param config 已验证搜索配置。 / Validated search configuration. -->
    ///
    /// # Returns
    /// 领域字段与匹配器的稳定映射。 / Stable mapping to domain field and matcher.
    ///
    /// <!-- @return 领域字段与匹配器的稳定映射。 / Stable mapping to domain field and matcher. -->
    const fn from_config(config: &crate::infrastructure::config::SearchConfig) -> Self {
        use crate::infrastructure::config::{SearchField as Field, SearchMatcher as Matcher};
        let default_field = match config.field {
            Field::Title => SearchField::Title,
            Field::Content => SearchField::Content,
            Field::Mixed => SearchField::Mixed,
        };
        let matcher = match config.matcher {
            Matcher::Fuzzy => SearchMatcher::Fuzzy,
            Matcher::Exact => SearchMatcher::Exact,
        };
        Self {
            default_field,
            matcher,
        }
    }
}

/// 单次求值共享的 XML 常驻内存预算。 / XML resident-memory budget shared by one evaluation.
///
/// <!-- @brief 单次求值共享的 XML 常驻内存预算。 / XML resident-memory budget shared by one evaluation. -->
struct XmlMemoryBudget {
    /// 尚可常驻内存的 XML 字节数。 / XML bytes still allowed to remain resident.
    ///
    /// <!-- @brief 尚可常驻内存的 XML 字节数。 / XML bytes still allowed to remain resident. -->
    remaining: usize,
}

impl XmlMemoryBudget {
    /// 创建默认的 16 MiB 聚合预算。 / Create the default aggregate 16 MiB budget.
    ///
    /// <!-- @brief 创建默认的 16 MiB 聚合预算。 / Create the default aggregate 16 MiB budget. -->
    ///
    /// # Returns
    /// 空预算计数器。 / Empty budget counter.
    ///
    /// <!-- @return 空预算计数器。 / Empty budget counter. -->
    const fn new() -> Self {
        Self {
            remaining: crate::application::value::XML_MEMORY_LIMIT,
        }
    }

    /// 为下一个 XML 值创建受剩余额度约束的写入器。 / Create a writer constrained by the remaining allowance.
    ///
    /// <!-- @brief 为下一个 XML 值创建受剩余额度约束的写入器。 / Create a writer constrained by the remaining allowance. -->
    ///
    /// # Returns
    /// 不会令该次求值总常驻 XML 超额的写入器。 / Writer that cannot exceed this evaluation's resident XML allowance.
    ///
    /// <!-- @return 不会令该次求值总常驻 XML 超额的写入器。 / Writer that cannot exceed this evaluation's resident XML allowance. -->
    fn writer(&self) -> SpillWriter {
        SpillWriter::with_threshold(self.remaining)
    }

    /// 记账一个完成的 XML 值。 / Account for one completed XML value.
    ///
    /// <!-- @brief 记账一个完成的 XML 值。 / Account for one completed XML value. -->
    ///
    /// # Arguments
    /// - `xml`: 已完成、即将进入返回值的 XML。 / Completed XML about to enter returned values.
    /// <!-- @param xml 已完成、即将进入返回值的 XML。 / Completed XML about to enter returned values. -->
    fn account(&mut self, xml: &crate::application::CanonicalXml) {
        self.remaining = self.remaining.saturating_sub(xml.resident_len());
    }
}

#[derive(Clone)]
/// 已准备正文及其锁外身份。 / Prepared body and its identity captured outside the writer lock.
///
/// <!-- @brief 已准备正文及其锁外身份。 / Prepared body and its identity captured outside the writer lock. -->
struct PreparedFragment {
    /// 该操作在源码位置上的目标符号。 / Target symbol at this operation's source position.
    ///
    /// <!-- @brief 该操作在源码位置上的目标符号。 / Target symbol at this operation's source position. -->
    target: Symbol,
    /// 编辑开始时的稳定节点身份。 / Stable node identity at edit start.
    ///
    /// <!-- @brief 编辑开始时的稳定节点身份。 / Stable node identity at edit start. -->
    edit_target: EditTarget,
    /// 已验证正文。 / Validated body.
    ///
    /// <!-- @brief 已验证正文。 / Validated body. -->
    text: XmlText,
}

#[derive(Clone)]
/// 准备阶段的顺序片段覆盖项。 / Sequential fragment-overlay entry during preparation.
///
/// <!-- @brief 准备阶段的顺序片段覆盖项。 / Sequential fragment-overlay entry during preparation. -->
struct StagedFragment {
    /// 首次持久化来源身份。 / Identity of the original durable source.
    ///
    /// <!-- @brief 首次持久化来源身份。 / Identity of the original durable source. -->
    target: EditTarget,
    /// 前序编辑产生的当前草稿。 / Current draft produced by preceding edits.
    ///
    /// <!-- @brief 前序编辑产生的当前草稿。 / Current draft produced by preceding edits. -->
    text: String,
}

/// 仅解析并按当前快照检查源码。 / Parse and check source against the current snapshot only.
///
/// <!-- @brief 仅解析并按当前快照检查源码。 / Parse and check source against the current snapshot only. -->
///
/// # Arguments
/// - `app`: 应用门面。 / Application facade.
/// <!-- @param app 应用门面。 / Application facade. -->
/// - `source`: DSL 源码。 / DSL source.
/// <!-- @param source DSL 源码。 / DSL source. -->
/// - `policy`: 调用能力策略。 / Invocation capability policy.
/// <!-- @param policy 调用能力策略。 / Invocation capability policy. -->
///
/// # Returns
/// 已检查程序。 / Checked program.
///
/// <!-- @return 已检查程序。 / Checked program. -->
///
/// # Errors
/// 当源码不完整或无效、快照读取失败、语义检查失败，或策略拒绝所需能力时返回诊断。 /
/// Returns a diagnostic when the source is incomplete or invalid, snapshot loading fails, semantic
/// validation fails, or policy denies a required capability.
pub(crate) fn check(
    app: &Promptr,
    source: &str,
    policy: InvocationPolicy,
) -> Result<CheckedProgram> {
    let ast = parse_complete(source)?;
    let snapshot = app.database.snapshot()?;
    crate::language::compile(&ast, &snapshot, policy)
}

/// 通过共享运行时执行一段源码。 / Execute source through the shared runtime.
///
/// <!-- @brief 通过共享运行时执行一段源码。 / Execute source through the shared runtime. -->
///
/// # Arguments
/// - `app`: 应用门面。 / Application facade.
/// <!-- @param app 应用门面。 / Application facade. -->
/// - `source`: DSL 源码。 / DSL source.
/// <!-- @param source DSL 源码。 / DSL source. -->
/// - `policy`: 调用能力策略。 / Invocation capability policy.
/// <!-- @param policy 调用能力策略。 / Invocation capability policy. -->
/// - `provider`: 可选交互文本提供者。 / Optional interactive text provider.
/// <!-- @param provider 可选交互文本提供者。 / Optional interactive text provider. -->
/// - `preconditions`: 调用方观察到的节点修订前置条件。 / Node revision preconditions observed by the caller.
/// <!-- @param preconditions 调用方观察到的节点修订前置条件。 / Node revision preconditions observed by the caller. -->
///
/// # Returns
/// 提交后可发布的类型化值。 / Typed values publishable after commit.
///
/// <!-- @return 提交后可发布的类型化值。 / Typed values publishable after commit. -->
///
/// # Errors
/// 当解析、快照读取、前置条件、编译、文本准备、事务或解释任一阶段失败时返回诊断。 /
/// Returns a diagnostic when parsing, snapshot loading, precondition validation, compilation, text
/// preparation, transaction handling, or interpretation fails.
pub(crate) fn eval(
    app: &mut Promptr,
    source: &str,
    policy: InvocationPolicy,
    provider: Option<&mut dyn TextProvider>,
    preconditions: &[NodePrecondition],
) -> Result<Vec<Value>> {
    let ast = parse_complete(source)?;
    let advisory_snapshot = app.database.snapshot()?;
    validate_preconditions(&advisory_snapshot, preconditions)?;
    let checked = crate::language::compile(&ast, &advisory_snapshot, policy)?;
    let Some(prepared) = prepare_fragments(&checked, &advisory_snapshot, provider)? else {
        return Ok(Vec::new());
    };
    let search_settings = SearchSettings::from_config(&app.config.search);
    if !checked.is_mutating() {
        return interpret_read_only(&checked, &advisory_snapshot, search_settings);
    }
    let mut operation = |transaction: &mut dyn CatalogWrite| {
        let authoritative = transaction.snapshot()?;
        validate_preconditions(&authoritative, preconditions)?;
        let mut program = crate::language::compile(&ast, &authoritative, policy)?;
        inject_prepared(&mut program, &prepared, &authoritative)?;
        interpret_transaction(&program, transaction, &authoritative, search_settings)
    };
    app.database.write_transaction(&mut operation)
}

/// 在单一一致快照上验证全部节点前置条件。 / Validate all node preconditions against one consistent snapshot.
///
/// <!-- @brief 在单一一致快照上验证全部节点前置条件。 / Validate all node preconditions against one consistent snapshot. -->
///
/// # Arguments
/// - `snapshot`: 读操作的一致快照或写事务中的权威快照。 / Consistent read snapshot or authoritative snapshot inside a write transaction.
/// <!-- @param snapshot 读操作的一致快照或写事务中的权威快照。 / Consistent read snapshot or authoritative snapshot inside a write transaction. -->
/// - `preconditions`: 调用方观察到的条件。 / Conditions observed by the caller.
/// <!-- @param preconditions 调用方观察到的条件。 / Conditions observed by the caller. -->
///
/// # Returns
/// 全部匹配时成功，否则返回稳定的 `E_CONFLICT`。 / Success when all match, otherwise stable `E_CONFLICT`.
///
/// <!-- @return 全部匹配时成功，否则返回稳定的 `E_CONFLICT`。 / Success when all match, otherwise stable `E_CONFLICT`. -->
///
/// # Errors
/// 任一符号不再绑定到调用方观察到的节点标识和修订时返回 `E_CONFLICT`。 /
/// Returns `E_CONFLICT` when any symbol no longer maps to the node identity and revision observed by
/// the caller.
fn validate_preconditions(
    snapshot: &CatalogSnapshot,
    preconditions: &[NodePrecondition],
) -> Result<()> {
    for condition in preconditions {
        let actual = snapshot.get_by_symbol(condition.symbol());
        if actual.is_some_and(|node| {
            node.header.id == condition.node_id() && node.header.revision == condition.revision()
        }) {
            continue;
        }
        let actual_description = actual.map_or_else(
            || "missing or renamed".to_owned(),
            |node| {
                format!(
                    "node id {}, revision {}",
                    node.header.id.get(),
                    node.header.revision.get()
                )
            },
        );
        return Err(Diagnostic::error(
            "E_CONFLICT",
            DiagnosticCategory::Conflict,
            format!(
                "node `{}` changed (expected node id {}, revision {}; found {actual_description})",
                condition.symbol(),
                condition.node_id().get(),
                condition.revision().get()
            ),
        ));
    }
    Ok(())
}

/// 把解析器状态稳定映射为公共诊断。 / Map parser states to stable public diagnostics.
///
/// <!-- @brief 把解析器状态稳定映射为公共诊断。 / Map parser states to stable public diagnostics. -->
///
/// # Errors
/// 当源码不完整或语法无效时返回带源码区间的稳定诊断。 /
/// Returns a stable diagnostic with a source span when the source is incomplete or syntactically
/// invalid.
fn parse_complete(source: &str) -> Result<Program> {
    let convert = |code: String, parsed: crate::language::ParseDiagnostic| {
        Diagnostic::error(code, DiagnosticCategory::Syntax, parsed.message)
            .with_span(SourceSpan::new(parsed.span.start, parsed.span.end))
            .with_cause(parsed.code.as_str())
    };
    match parse(source) {
        ParseOutcome::Complete(program) => Ok(program),
        ParseOutcome::Incomplete(parsed) => Err(convert("E_INCOMPLETE".to_owned(), parsed)
            .with_hint("append the missing DSL input and try again")),
        ParseOutcome::Invalid(parsed) => {
            let code = parsed.code.as_str().to_owned();
            Err(convert(code, parsed))
        }
    }
}

/// 写锁外准备所有片段文本。 / Prepare all fragment text outside the writer lock.
///
/// <!-- @brief 写锁外准备所有片段文本。 / Prepare all fragment text outside the writer lock. -->
///
/// # Returns
/// `None` 表示用户取消整个调用。 / `None` means the user cancelled the whole invocation.
///
/// <!-- @return `None` 表示用户取消整个调用。 / `None` means the user cancelled the whole invocation. -->
///
/// # Errors
/// 当缺少文本提供者、编辑器失败或篡改编辑身份，或返回正文不是有效 XML 文本时返回诊断。 /
/// Returns a diagnostic when no text provider is available, the editor fails or changes edit
/// identity, or the returned body is not valid XML text.
fn prepare_fragments(
    program: &CheckedProgram,
    snapshot: &CatalogSnapshot,
    provider: Option<&mut dyn TextProvider>,
) -> Result<Option<Vec<PreparedFragment>>> {
    let count = program
        .ops
        .iter()
        .filter(|op| matches!(op, Op::UpsertFragment { .. }))
        .count();
    if count == 0 {
        return Ok(Some(Vec::new()));
    }
    let provider = provider.ok_or_else(|| {
        Diagnostic::error(
            "E_TEXT_PROVIDER",
            DiagnosticCategory::External,
            "FRAGMENT requires a text provider",
        )
    })?;
    let mut staged = snapshot
        .iter_by_symbol()
        .filter_map(|node| {
            fragment_text(node).map(|text| {
                (
                    node.header.symbol.clone(),
                    StagedFragment {
                        target: EditTarget {
                            node_id: Some(node.header.id),
                            base_revision: Some(node.header.revision),
                        },
                        text: text.to_owned(),
                    },
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    let mut prepared = Vec::with_capacity(count);
    for op in &program.ops {
        match op {
            Op::UpsertFragment { target, .. } => {
                let previous = staged.get(target).cloned().unwrap_or(StagedFragment {
                    target: EditTarget {
                        node_id: None,
                        base_revision: None,
                    },
                    text: String::new(),
                });
                let request = EditRequest {
                    target: previous.target,
                    original_text: previous.text,
                };
                let outcome = provider.edit(request.clone()).map_err(|error| {
                    Diagnostic::error(
                        "E_EDITOR",
                        DiagnosticCategory::External,
                        "text provider failed",
                    )
                    .with_cause(error.to_string())
                })?;
                let EditorOutcome::Save(edit) = outcome else {
                    return Ok(None);
                };
                let returned_identity_is_valid = matches!(
                    (edit.target.node_id, edit.target.base_revision),
                    (Some(_), Some(_)) | (None, None)
                );
                if !returned_identity_is_valid || edit.target.node_id != request.target.node_id {
                    return Err(Diagnostic::error(
                        "E_EDITOR_TARGET",
                        DiagnosticCategory::External,
                        "text provider changed the edit identity",
                    ));
                }
                let text = XmlText::new(edit.edited_text).map_err(domain_diagnostic)?;
                staged.insert(
                    target.clone(),
                    StagedFragment {
                        target: edit.target,
                        text: text.as_str().to_owned(),
                    },
                );
                prepared.push(PreparedFragment {
                    target: target.clone(),
                    edit_target: edit.target,
                    text,
                });
            }
            Op::Rename { target, new_symbol } => {
                if let Some(fragment) = staged.remove(target) {
                    staged.insert(new_symbol.clone(), fragment);
                }
            }
            Op::Delete { target } => {
                staged.remove(target);
            }
            _ => {}
        }
    }
    Ok(Some(prepared))
}

/// 将锁外准备的正文注入权威重编译结果。 / Inject prepared bodies into the authoritative recompilation.
///
/// <!-- @brief 将锁外准备的正文注入权威重编译结果。 / Inject prepared bodies into the authoritative recompilation. -->
///
/// # Errors
/// 当准备结果与重编译程序不一致，或编辑期间节点身份发生变化时返回诊断。 /
/// Returns a diagnostic when prepared results do not match the recompiled program or a node identity
/// changed during editing.
fn inject_prepared(
    program: &mut CheckedProgram,
    prepared: &[PreparedFragment],
    authoritative: &CatalogSnapshot,
) -> Result<()> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Identity {
        Existing(crate::domain::NodeId, crate::domain::Revision),
        Created,
    }
    let mut identities = authoritative
        .iter_by_symbol()
        .map(|node| {
            (
                node.header.symbol.clone(),
                Identity::Existing(node.header.id, node.header.revision),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut edits = prepared.iter();
    for op in &mut program.ops {
        match op {
            Op::UpsertFragment {
                text,
                expected_revision,
                target,
            } => {
                let edit = edits.next().ok_or_else(internal_program_mismatch)?;
                if edit.target != *target {
                    return Err(internal_program_mismatch());
                }
                let expected_identity =
                    match (edit.edit_target.node_id, edit.edit_target.base_revision) {
                        (Some(id), Some(revision)) => Some(Identity::Existing(id, revision)),
                        (None, None) => None,
                        _ => return Err(internal_program_mismatch()),
                    };
                let actual = identities.get(target).copied();
                let identity_matches = match expected_identity {
                    Some(expected) => actual == Some(expected),
                    None => actual.is_none() || actual == Some(Identity::Created),
                };
                if !identity_matches {
                    return Err(Diagnostic::error(
                        "E_CONFLICT",
                        DiagnosticCategory::Conflict,
                        format!("fragment `{target}` changed while it was being edited"),
                    ));
                }
                identities
                    .entry(target.clone())
                    .or_insert(Identity::Created);
                *text = Some(edit.text.clone());
                *expected_revision = edit.edit_target.base_revision;
            }
            Op::Rename { target, new_symbol } => {
                if let Some(identity) = identities.remove(target) {
                    identities.insert(new_symbol.clone(), identity);
                }
            }
            Op::Delete { target } => {
                identities.remove(target);
            }
            Op::ReplacePrompt { target, .. } => {
                identities
                    .entry(target.clone())
                    .or_insert(Identity::Created);
            }
            _ => {}
        }
    }
    if edits.next().is_some() {
        return Err(internal_program_mismatch());
    }
    Ok(())
}

/// 在一个不可变快照上执行纯查询。 / Execute pure queries on one immutable snapshot.
///
/// <!-- @brief 在一个不可变快照上执行纯查询。 / Execute pure queries on one immutable snapshot. -->
///
/// # Errors
/// 当任一查询违反目录不变量、引用未知节点，或 XML 渲染与落盘失败时返回诊断。 /
/// Returns a diagnostic when any query violates catalog invariants, references an unknown node, or
/// XML rendering or spilling fails.
fn interpret_read_only(
    program: &CheckedProgram,
    snapshot: &CatalogSnapshot,
    search_settings: SearchSettings,
) -> Result<Vec<Value>> {
    let mut budget = XmlMemoryBudget::new();
    program
        .ops
        .iter()
        .map(|op| interpret_query(op, snapshot, &mut budget, search_settings))
        .collect()
}

/// 在事务内按源码顺序执行并缓冲结果。 / Execute in source order and buffer results inside a transaction.
///
/// <!-- @brief 在事务内按源码顺序执行并缓冲结果。 / Execute in source order and buffer results inside a transaction. -->
///
/// # Errors
/// 当持久化操作、修订推进、事务内快照读取或查询解释失败时返回诊断。 /
/// Returns a diagnostic when persistence, revision advancement, transactional snapshot loading, or
/// query interpretation fails.
fn interpret_transaction(
    program: &CheckedProgram,
    transaction: &mut dyn CatalogWrite,
    authoritative: &CatalogSnapshot,
    search_settings: SearchSettings,
) -> Result<Vec<Value>> {
    let mut values = Vec::with_capacity(program.ops.len());
    let mut budget = XmlMemoryBudget::new();
    let mut state = authoritative
        .iter_by_symbol()
        .map(|node| {
            (
                node.header.symbol.clone(),
                (node.header.revision, node.header.metadata.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for op in &program.ops {
        let value = match op {
            Op::UpsertFragment {
                target,
                text,
                expected_revision,
            } => {
                let current_revision = state.get(target).map(|entry| entry.0);
                // 协调器已将准备基线与修改前权威快照比较；此处当前修订可合法包含同一脚本的前序操作。
                // The coordinator already compared the prepared base with the authoritative
                // pre-mutation snapshot; the current revision may include earlier script operations.
                let _ = expected_revision;
                transaction.upsert_fragment(
                    target,
                    text.as_ref().ok_or_else(internal_program_mismatch)?,
                    current_revision,
                )?;
                advance_state(&mut state, target)?;
                Value::Unit
            }
            Op::ReplacePrompt { target, children } => {
                transaction.replace_prompt(target, children)?;
                advance_state(&mut state, target)?;
                Value::Unit
            }
            Op::Rename { target, new_symbol } => {
                transaction.rename(target, new_symbol)?;
                let (revision, metadata) =
                    state.remove(target).ok_or_else(|| unknown_symbol(target))?;
                state.insert(new_symbol.clone(), (next_revision(revision)?, metadata));
                Value::Unit
            }
            Op::Delete { target } => {
                transaction.delete(target)?;
                state.remove(target);
                Value::Unit
            }
            Op::SetDescription {
                target,
                description,
            } => {
                let mut metadata = state
                    .get(target)
                    .map(|entry| entry.1.clone())
                    .ok_or_else(|| unknown_symbol(target))?;
                metadata.set_description(description.clone());
                transaction.set_metadata(target, &metadata)?;
                advance_existing_state(&mut state, target, metadata.clone())?;
                Value::Metadata(metadata)
            }
            Op::SetTags { target, tags } => {
                let parsed = tags
                    .iter()
                    .cloned()
                    .map(Tag::new)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(domain_diagnostic)?;
                let mut metadata = state
                    .get(target)
                    .map(|entry| entry.1.clone())
                    .ok_or_else(|| unknown_symbol(target))?;
                metadata.set_tags(parsed);
                transaction.set_metadata(target, &metadata)?;
                advance_existing_state(&mut state, target, metadata.clone())?;
                Value::Metadata(metadata)
            }
            query => {
                let snapshot = transaction.snapshot()?;
                interpret_query(query, &snapshot, &mut budget, search_settings)?
            }
        };
        values.push(value);
    }
    Ok(values)
}

/// 将节点状态推进一次领域修订。 / Advance node state by one domain revision.
///
/// <!-- @brief 将节点状态推进一次领域修订。 / Advance node state by one domain revision. -->
///
/// # Errors
/// 当现有修订溢出或无法构造新节点的初始修订时返回诊断。 /
/// Returns a diagnostic when an existing revision overflows or a new node's initial revision cannot
/// be constructed.
fn advance_state(
    state: &mut BTreeMap<Symbol, (crate::domain::Revision, Metadata)>,
    target: &Symbol,
) -> Result<()> {
    if let Some((revision, _)) = state.get_mut(target) {
        *revision = next_revision(*revision)?;
    } else {
        state.insert(
            target.clone(),
            (
                crate::domain::Revision::new(1).map_err(domain_diagnostic)?,
                Metadata::default(),
            ),
        );
    }
    Ok(())
}

/// 推进现有节点并替换缓存元数据。 / Advance an existing node and replace cached metadata.
///
/// <!-- @brief 推进现有节点并替换缓存元数据。 / Advance an existing node and replace cached metadata. -->
///
/// # Errors
/// 当目标不在事务状态中或其修订溢出时返回诊断。 /
/// Returns a diagnostic when the target is absent from transaction state or its revision overflows.
fn advance_existing_state(
    state: &mut BTreeMap<Symbol, (crate::domain::Revision, Metadata)>,
    target: &Symbol,
    metadata: Metadata,
) -> Result<()> {
    let entry = state
        .get_mut(target)
        .ok_or_else(|| unknown_symbol(target))?;
    entry.0 = next_revision(entry.0)?;
    entry.1 = metadata;
    Ok(())
}

/// 计算下一修订并映射溢出。 / Compute the next revision and map overflow.
///
/// <!-- @brief 计算下一修订并映射溢出。 / Compute the next revision and map overflow. -->
///
/// # Errors
/// 当修订号已达到其表示上限时返回 `E_REVISION_OVERFLOW`。 /
/// Returns `E_REVISION_OVERFLOW` when the revision has reached its representable maximum.
fn next_revision(revision: crate::domain::Revision) -> Result<crate::domain::Revision> {
    revision.checked_next().ok_or_else(|| {
        Diagnostic::error(
            "E_REVISION_OVERFLOW",
            DiagnosticCategory::Domain,
            "node revision overflow",
        )
    })
}

/// 解释一个无副作用操作。 / Interpret one side-effect-free operation.
///
/// <!-- @brief 解释一个无副作用操作。 / Interpret one side-effect-free operation. -->
///
/// # Errors
/// 当操作不是查询、引用未知或错误种类的节点、目录含悬空引用，或 XML 渲染与落盘失败时返回诊断。 /
/// Returns a diagnostic when the operation is not a query, references an unknown or wrong-kind node,
/// the catalog contains dangling references, or XML rendering or spilling fails.
fn interpret_query(
    op: &Op,
    snapshot: &CatalogSnapshot,
    budget: &mut XmlMemoryBudget,
    search_settings: SearchSettings,
) -> Result<Value> {
    match op {
        Op::List { filter } => Ok(Value::Nodes(
            snapshot
                .iter_by_symbol()
                .filter(|node| filter_matches(*filter, node.header.kind))
                .map(|node| node_view(snapshot, node))
                .collect::<Result<_>>()?,
        )),
        Op::Inspect { target } => Ok(Value::Node(node_view(
            snapshot,
            snapshot
                .get_by_symbol(target)
                .ok_or_else(|| unknown_symbol(target))?,
        )?)),
        Op::Search { query, field, root } => Ok(Value::SearchResults(search(
            snapshot,
            query,
            field.unwrap_or(search_settings.default_field),
            root.as_ref(),
            search_settings.matcher,
        )?)),
        Op::RenderXml { root } => {
            let node = snapshot
                .get_by_symbol(root)
                .ok_or_else(|| unknown_symbol(root))?;
            let mut writer = budget.writer();
            snapshot
                .render_xml(node.header.id, &mut writer)
                .map_err(|error| {
                    Diagnostic::error(
                        "E_RENDER",
                        DiagnosticCategory::Domain,
                        "canonical XML rendering failed",
                    )
                    .with_cause(error.to_string())
                })?;
            let xml = writer.finish().map_err(|error| {
                Diagnostic::error(
                    "E_RENDER_IO",
                    DiagnosticCategory::External,
                    "canonical XML spool finalization failed",
                )
                .with_cause(error.to_string())
            })?;
            budget.account(&xml);
            Ok(Value::Xml(xml))
        }
        _ => Err(internal_program_mismatch()),
    }
}

fn filter_matches(filter: NodeFilter, kind: NodeKind) -> bool {
    matches!(filter, NodeFilter::All)
        || matches!(
            (filter, kind),
            (NodeFilter::Fragments, NodeKind::Fragment) | (NodeFilter::Prompts, NodeKind::Prompt)
        )
}

fn node_view(snapshot: &CatalogSnapshot, node: &Node) -> Result<NodeView> {
    let (children, byte_size) = match &node.body {
        NodeBody::Fragment(text) => (Vec::new(), Some(text.as_str().len())),
        NodeBody::Prompt(children) => (
            children
                .as_slice()
                .iter()
                .map(|id| {
                    snapshot
                        .get(*id)
                        .map(|child| child.header.symbol.clone())
                        .ok_or_else(|| {
                            Diagnostic::error(
                                "E_DANGLING",
                                DiagnosticCategory::ReferentialIntegrity,
                                format!("missing child node {id}"),
                            )
                        })
                })
                .collect::<Result<Vec<_>>>()?,
            None,
        ),
    };
    Ok(NodeView {
        id: node.header.id,
        symbol: node.header.symbol.clone(),
        kind: node.header.kind,
        revision: node.header.revision,
        children,
        byte_size,
        metadata: node.header.metadata.clone(),
    })
}

fn search(
    snapshot: &CatalogSnapshot,
    query: &str,
    field: SearchField,
    root: Option<&Symbol>,
    matcher_kind: SearchMatcher,
) -> Result<Vec<SearchHit>> {
    let occurrences = if let Some(root) = root {
        let root = snapshot
            .get_by_symbol(root)
            .ok_or_else(|| unknown_symbol(root))?;
        if root.header.kind != NodeKind::Prompt {
            return Err(Diagnostic::error(
                "E_KIND",
                DiagnosticCategory::Domain,
                format!("`{}` is not a prompt", root.header.symbol),
            ));
        }
        snapshot
            .occurrence_counts_saturating(root.header.id)
            .map_err(domain_diagnostic)?
    } else {
        BTreeMap::new()
    };
    let mut matcher = Matcher::new(MatcherConfig::DEFAULT.match_paths());
    let mut needle_buf = Vec::new();
    let needle = Utf32Str::new(query, &mut needle_buf);
    let mut hits = Vec::new();
    for node in snapshot.iter_by_symbol() {
        let NodeBody::Fragment(text) = &node.body else {
            continue;
        };
        if root.is_some() && !occurrences.contains_key(&node.header.id) {
            continue;
        }
        if let Some(score) = match_score(
            &mut matcher,
            needle,
            query,
            node.header.symbol.as_str(),
            text.as_str(),
            field,
            matcher_kind,
        ) {
            hits.push(SearchHit {
                node_id: node.header.id,
                symbol: node.header.symbol.clone(),
                score,
                occurrence_count: occurrences.get(&node.header.id).copied().unwrap_or(1),
            });
        }
    }
    hits.sort_by(SearchHit::deterministic_cmp);
    Ok(hits)
}

fn match_score(
    matcher: &mut Matcher,
    needle: Utf32Str<'_>,
    query: &str,
    title: &str,
    content: &str,
    field: SearchField,
    matcher_kind: SearchMatcher,
) -> Option<f64> {
    fn one(matcher: &mut Matcher, needle: Utf32Str<'_>, value: &str) -> Option<u16> {
        let mut buffer = Vec::new();
        matcher.fuzzy_match(Utf32Str::new(value, &mut buffer), needle)
    }
    fn exact(needle: &str, value: &str) -> Option<f64> {
        value.contains(needle).then_some(1.0)
    }
    match matcher_kind {
        SearchMatcher::Exact => match field {
            SearchField::Title => exact(query, title),
            SearchField::Content => exact(query, content),
            SearchField::Mixed => exact(query, title).or_else(|| exact(query, content)),
        },
        SearchMatcher::Fuzzy => match field {
            SearchField::Title => one(matcher, needle, title).map(f64::from),
            SearchField::Content => one(matcher, needle, content).map(f64::from),
            SearchField::Mixed => {
                match (one(matcher, needle, title), one(matcher, needle, content)) {
                    (None, None) => None,
                    (a, b) => Some(f64::from(a.unwrap_or(0).saturating_add(b.unwrap_or(0)))),
                }
            }
        },
    }
}

fn fragment_text(node: &Node) -> Option<&str> {
    match &node.body {
        NodeBody::Fragment(text) => Some(text.as_str()),
        NodeBody::Prompt(_) => None,
    }
}
fn domain_diagnostic(error: DomainError) -> Diagnostic {
    let (code, category) = match &error {
        DomainError::InvalidPositiveInteger { .. } => {
            ("E_INVALID_INTEGER", DiagnosticCategory::Domain)
        }
        DomainError::InvalidSymbol(_) => ("E_INVALID_SYMBOL", DiagnosticCategory::Domain),
        DomainError::InvalidXmlCharacter { .. } => {
            ("E_INVALID_XML_TEXT", DiagnosticCategory::Domain)
        }
        DomainError::EmptyChildren => ("E_EMPTY_PROMPT", DiagnosticCategory::Domain),
        DomainError::EmptyTag => ("E_INVALID_TAG", DiagnosticCategory::Domain),
        DomainError::KindMismatch { .. } => ("E_NODE_KIND", DiagnosticCategory::Domain),
        DomainError::DuplicateNodeId(_) => ("E_DUPLICATE_NODE_ID", DiagnosticCategory::Domain),
        DomainError::DuplicateSymbol(_) => ("E_SYMBOL_EXISTS", DiagnosticCategory::Domain),
        DomainError::MissingNode(_) => ("E_REF_MISSING", DiagnosticCategory::ReferentialIntegrity),
        DomainError::CycleDetected { .. } => ("E_CYCLE", DiagnosticCategory::ReferentialIntegrity),
        DomainError::OccurrenceOverflow { .. } => {
            ("E_OCCURRENCE_OVERFLOW", DiagnosticCategory::Domain)
        }
    };
    Diagnostic::error(code, category, error.to_string())
}
fn unknown_symbol(symbol: &Symbol) -> Diagnostic {
    Diagnostic::error(
        "E_UNKNOWN_SYMBOL",
        DiagnosticCategory::ReferentialIntegrity,
        format!("unknown symbol `{symbol}`"),
    )
}
fn internal_program_mismatch() -> Diagnostic {
    Diagnostic::error(
        "E_INTERNAL_PROGRAM",
        DiagnosticCategory::Internal,
        "checked program and runtime preparation disagree",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::ports::{CatalogRead, Database},
        domain::{NodeHeader, NodeId, Revision},
        infrastructure::{
            config::{Config, ConfigPaths},
            editor::{EditorError, PreparedEdit},
        },
    };
    use std::{cell::RefCell, path::PathBuf, rc::Rc};

    #[derive(Default)]
    struct Shared {
        nodes: Vec<Node>,
        transactions: usize,
        fail_commit: bool,
        failed_spool: Option<PathBuf>,
        before_transaction: Option<ConcurrentChange>,
    }

    /// 模拟另一连接在建议快照之后提交的变化。 / Simulated change committed by another connection after the advisory snapshot.
    ///
    /// <!-- @brief 模拟另一连接在建议快照之后提交的变化。 / Simulated change committed by another connection after the advisory snapshot. -->
    enum ConcurrentChange {
        /// 推进指定节点修订号。 / Advance the named node revision.
        ///
        /// <!-- @brief 推进指定节点修订号。 / Advance the named node revision. -->
        Revise(Symbol),
        /// 改变指定节点的符号绑定。 / Change the named node's symbol binding.
        ///
        /// <!-- @brief 改变指定节点的符号绑定。 / Change the named node's symbol binding. -->
        Rename(Symbol, Symbol),
    }
    struct FakeDatabase(Rc<RefCell<Shared>>);
    struct FakeWrite {
        nodes: Vec<Node>,
    }

    impl CatalogRead for FakeDatabase {
        fn snapshot(&self) -> Result<CatalogSnapshot> {
            CatalogSnapshot::new(self.0.borrow().nodes.clone()).map_err(domain_diagnostic)
        }
    }
    impl Database for FakeDatabase {
        fn write_transaction(
            &mut self,
            operation: &mut dyn FnMut(&mut dyn CatalogWrite) -> Result<Vec<Value>>,
        ) -> Result<Vec<Value>> {
            {
                let mut shared = self.0.borrow_mut();
                shared.transactions += 1;
                match shared.before_transaction.take() {
                    Some(ConcurrentChange::Revise(symbol)) => {
                        let node = shared
                            .nodes
                            .iter_mut()
                            .find(|node| node.header.symbol == symbol)
                            .expect("simulated concurrent target must exist");
                        node.header.revision = node.header.revision.checked_next().unwrap();
                    }
                    Some(ConcurrentChange::Rename(from, to)) => {
                        let node = shared
                            .nodes
                            .iter_mut()
                            .find(|node| node.header.symbol == from)
                            .expect("simulated concurrent target must exist");
                        node.header.symbol = to;
                    }
                    None => {}
                }
            }
            let mut writer = FakeWrite {
                nodes: self.0.borrow().nodes.clone(),
            };
            let values = operation(&mut writer)?;
            if self.0.borrow().fail_commit {
                self.0.borrow_mut().failed_spool = values.iter().find_map(|value| match value {
                    Value::Xml(xml) => xml.spilled_path(),
                    _ => None,
                });
                return Err(Diagnostic::error(
                    "E_COMMIT",
                    DiagnosticCategory::Storage,
                    "injected commit failure",
                ));
            }
            self.0.borrow_mut().nodes = writer.nodes;
            Ok(values)
        }
    }
    impl CatalogRead for FakeWrite {
        fn snapshot(&self) -> Result<CatalogSnapshot> {
            CatalogSnapshot::new(self.nodes.clone()).map_err(domain_diagnostic)
        }
    }
    impl CatalogWrite for FakeWrite {
        fn upsert_fragment(&mut self, _: &Symbol, _: &XmlText, _: Option<Revision>) -> Result<()> {
            unsupported()
        }
        fn replace_prompt(&mut self, _: &Symbol, _: &[Symbol]) -> Result<()> {
            unsupported()
        }
        fn rename(&mut self, _: &Symbol, _: &Symbol) -> Result<()> {
            unsupported()
        }
        fn delete(&mut self, _: &Symbol) -> Result<()> {
            unsupported()
        }
        fn set_metadata(&mut self, target: &Symbol, metadata: &Metadata) -> Result<()> {
            let node = self
                .nodes
                .iter_mut()
                .find(|node| &node.header.symbol == target)
                .ok_or_else(|| unknown_symbol(target))?;
            node.header.metadata = metadata.clone();
            node.header.revision = node
                .header
                .revision
                .checked_next()
                .ok_or_else(internal_program_mismatch)?;
            Ok(())
        }
    }
    fn unsupported<T>() -> Result<T> {
        Err(Diagnostic::error(
            "E_TEST",
            DiagnosticCategory::Internal,
            "unsupported fake operation",
        ))
    }
    fn fragment(id: i64, symbol: &str, text: &str) -> Node {
        let body = NodeBody::Fragment(XmlText::new(text).unwrap());
        Node::from_parts(
            NodeHeader {
                id: NodeId::new(id).unwrap(),
                symbol: Symbol::new(symbol).unwrap(),
                kind: NodeKind::Fragment,
                revision: Revision::new(1).unwrap(),
                metadata: Metadata::default(),
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            body,
        )
        .unwrap()
    }
    fn prompt(id: i64, symbol: &str, children: Vec<NodeId>) -> Node {
        let body = NodeBody::Prompt(crate::domain::NonEmptyChildren::new(children).unwrap());
        Node::from_parts(
            NodeHeader {
                id: NodeId::new(id).unwrap(),
                symbol: Symbol::new(symbol).unwrap(),
                kind: NodeKind::Prompt,
                revision: Revision::new(1).unwrap(),
                metadata: Metadata::default(),
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            body,
        )
        .unwrap()
    }
    fn config() -> Config {
        Config::defaults(&ConfigPaths {
            user_config: PathBuf::new(),
            database: PathBuf::new(),
            state_dir: PathBuf::new(),
            cache_dir: PathBuf::new(),
            backup_dir: PathBuf::new(),
        })
    }
    fn app(nodes: Vec<Node>, fail_commit: bool) -> (Promptr, Rc<RefCell<Shared>>) {
        let shared = Rc::new(RefCell::new(Shared {
            nodes,
            transactions: 0,
            fail_commit,
            failed_spool: None,
            before_transaction: None,
        }));
        (
            Promptr::from_parts(Box::new(FakeDatabase(shared.clone())), config()),
            shared,
        )
    }
    #[derive(Default)]
    struct CountingProvider {
        calls: usize,
    }
    impl TextProvider for CountingProvider {
        fn edit(
            &mut self,
            request: EditRequest,
        ) -> std::result::Result<EditorOutcome, EditorError> {
            self.calls += 1;
            Ok(EditorOutcome::Save(PreparedEdit::from_request(
                request,
                "draft".into(),
            )))
        }
    }

    struct SequenceProvider {
        originals: Vec<String>,
        replacements: std::collections::VecDeque<String>,
    }
    impl TextProvider for SequenceProvider {
        fn edit(
            &mut self,
            request: EditRequest,
        ) -> std::result::Result<EditorOutcome, EditorError> {
            self.originals.push(request.original_text.clone());
            Ok(EditorOutcome::Save(PreparedEdit::from_request(
                request,
                self.replacements.pop_front().unwrap(),
            )))
        }
    }

    /// 返回同一节点的旧修订身份。 / Return a stale revision identity for the same node.
    ///
    /// <!-- @brief 返回同一节点的旧修订身份。 / Return a stale revision identity for the same node. -->
    struct StaleIdentityProvider {
        revision: Revision,
    }

    impl TextProvider for StaleIdentityProvider {
        fn edit(
            &mut self,
            request: EditRequest,
        ) -> std::result::Result<EditorOutcome, EditorError> {
            Ok(EditorOutcome::Save(PreparedEdit {
                target: EditTarget {
                    node_id: request.target.node_id,
                    base_revision: Some(self.revision),
                },
                original_text: request.original_text,
                edited_text: "draft".to_owned(),
            }))
        }
    }

    #[test]
    fn read_queries_share_snapshot_and_expose_typed_output() {
        let (mut app, shared) = app(vec![fragment(1, "Leaf", "<&")], false);
        let values = app
            .eval(
                "LIST FRAGMENTS; PRINT Leaf; SEARCH \"Le\" FROM TITLE; OUTPUT Leaf;",
                InvocationPolicy::script(),
            )
            .unwrap();
        assert!(
            matches!(&values[0], Value::Nodes(nodes) if nodes.len() == 1 && nodes[0].byte_size == Some(2))
        );
        assert!(matches!(&values[1], Value::Node(node) if node.symbol.as_str() == "Leaf"));
        assert!(matches!(&values[2], Value::SearchResults(hits) if hits.len() == 1));
        assert_eq!(values[3], Value::Xml("<Leaf>&lt;&amp;</Leaf>\n".into()));
        assert_eq!(shared.borrow().transactions, 0);
    }

    #[test]
    fn configured_search_field_applies_only_when_from_is_omitted() {
        let (mut app, _) = app(vec![fragment(1, "NeedleTitle", "plain body")], false);
        app.config.search.field = crate::infrastructure::config::SearchField::Content;

        let values = app
            .eval(
                "SEARCH \"NeedleTitle\"; SEARCH \"NeedleTitle\" FROM TITLE;",
                InvocationPolicy::script(),
            )
            .unwrap();
        assert!(matches!(&values[0], Value::SearchResults(hits) if hits.is_empty()));
        assert!(matches!(&values[1], Value::SearchResults(hits) if hits.len() == 1));
    }

    #[test]
    fn exact_search_is_a_case_sensitive_substring_not_a_fuzzy_match() {
        let nodes = vec![fragment(1, "needletitle", "plain body")];
        let (mut fuzzy, _) = app(nodes.clone(), false);
        fuzzy.config.search.matcher = crate::infrastructure::config::SearchMatcher::Fuzzy;
        let fuzzy_values = fuzzy
            .eval("SEARCH \"ndl\" FROM TITLE;", InvocationPolicy::script())
            .unwrap();
        assert!(matches!(&fuzzy_values[0], Value::SearchResults(hits) if hits.len() == 1));

        let (mut exact, _) = app(nodes, false);
        exact.config.search.matcher = crate::infrastructure::config::SearchMatcher::Exact;
        let exact_values = exact
            .eval(
                "SEARCH \"ndl\" FROM TITLE; SEARCH \"needle\" FROM TITLE; SEARCH \"Needle\" FROM TITLE;",
                InvocationPolicy::script(),
            )
            .unwrap();
        assert!(matches!(&exact_values[0], Value::SearchResults(hits) if hits.is_empty()));
        assert!(
            matches!(&exact_values[1], Value::SearchResults(hits) if hits.len() == 1 && hits[0].score == 1.0)
        );
        assert!(matches!(&exact_values[2], Value::SearchResults(hits) if hits.is_empty()));
    }

    #[test]
    fn find_uses_configured_matching_while_preserving_reachable_scope() {
        let nodes = vec![
            fragment(1, "Inside", "exact needle"),
            fragment(2, "Outside", "exact needle"),
            prompt(3, "Root", vec![NodeId::new(1).unwrap()]),
        ];
        let (mut app, _) = app(nodes, false);
        app.config.search.field = crate::infrastructure::config::SearchField::Content;
        app.config.search.matcher = crate::infrastructure::config::SearchMatcher::Exact;

        let values = app
            .eval("FIND \"needle\" ON Root;", InvocationPolicy::script())
            .unwrap();
        assert!(matches!(
            &values[0],
            Value::SearchResults(hits)
                if hits.len() == 1 && hits[0].symbol.as_str() == "Inside"
        ));
    }

    #[test]
    fn capability_rejection_precedes_provider_and_transaction() {
        let (mut app, shared) = app(Vec::new(), false);
        let mut provider = CountingProvider::default();
        let error = app
            .eval_with_provider("FRAGMENT New;", InvocationPolicy::script(), &mut provider)
            .unwrap_err();
        assert_eq!(error.code, "E0201");
        assert_eq!((provider.calls, shared.borrow().transactions), (0, 0));
    }

    #[test]
    fn stale_precondition_rejects_whole_batch_before_output_or_mutation() {
        let leaf = fragment(1, "Leaf", "body");
        let condition = NodePrecondition::new(
            leaf.header.symbol.clone(),
            leaf.header.id,
            leaf.header.revision,
        );
        let (mut app, shared) = app(vec![leaf], false);
        shared.borrow_mut().before_transaction =
            Some(ConcurrentChange::Revise(Symbol::new("Leaf").unwrap()));

        let error = app
            .eval_preconditioned(
                "OUTPUT Leaf; METADATA Leaf DESCRIPTION \"ours\";",
                InvocationPolicy::script(),
                &[condition],
            )
            .unwrap_err();

        assert_eq!(error.code, "E_CONFLICT");
        let state = shared.borrow();
        assert_eq!(state.transactions, 1);
        assert_eq!(state.nodes[0].header.revision.get(), 2);
        assert_eq!(state.nodes[0].header.metadata.description(), None);
    }

    #[test]
    fn renamed_precondition_target_reports_conflict_before_authoritative_compile() {
        let leaf = fragment(1, "Leaf", "body");
        let condition = NodePrecondition::new(
            leaf.header.symbol.clone(),
            leaf.header.id,
            leaf.header.revision,
        );
        let (mut app, shared) = app(vec![leaf], false);
        shared.borrow_mut().before_transaction = Some(ConcurrentChange::Rename(
            Symbol::new("Leaf").unwrap(),
            Symbol::new("OtherName").unwrap(),
        ));

        let error = app
            .eval_preconditioned(
                "METADATA Leaf DESCRIPTION \"ours\";",
                InvocationPolicy::script(),
                &[condition],
            )
            .unwrap_err();

        assert_eq!(error.code, "E_CONFLICT");
        let state = shared.borrow();
        assert_eq!(state.nodes[0].header.symbol.as_str(), "OtherName");
        assert_eq!(state.nodes[0].header.metadata.description(), None);
    }

    #[test]
    fn unrelated_concurrent_change_does_not_reject_precondition() {
        let leaf = fragment(1, "Leaf", "body");
        let condition = NodePrecondition::new(
            leaf.header.symbol.clone(),
            leaf.header.id,
            leaf.header.revision,
        );
        let (mut app, shared) = app(vec![leaf, fragment(2, "Other", "other")], false);
        shared.borrow_mut().before_transaction =
            Some(ConcurrentChange::Revise(Symbol::new("Other").unwrap()));

        let values = app
            .eval_preconditioned(
                "METADATA Leaf DESCRIPTION \"ours\";",
                InvocationPolicy::script(),
                &[condition],
            )
            .unwrap();

        assert!(!values.is_empty());
        let state = shared.borrow();
        let leaf = state
            .nodes
            .iter()
            .find(|node| node.header.symbol.as_str() == "Leaf")
            .unwrap();
        let other = state
            .nodes
            .iter()
            .find(|node| node.header.symbol.as_str() == "Other")
            .unwrap();
        assert_eq!(leaf.header.metadata.description(), Some("ours"));
        assert_eq!(leaf.header.revision.get(), 2);
        assert_eq!(other.header.revision.get(), 2);
    }

    #[test]
    fn read_only_precondition_is_checked_on_its_consistent_snapshot() {
        let leaf = fragment(1, "Leaf", "body");
        let condition = NodePrecondition::new(
            leaf.header.symbol.clone(),
            leaf.header.id,
            leaf.header.revision,
        );
        let (mut app, shared) = app(vec![leaf], false);
        shared.borrow_mut().nodes[0].header.revision = Revision::new(2).unwrap();

        let error = app
            .eval_preconditioned("OUTPUT Leaf;", InvocationPolicy::script(), &[condition])
            .unwrap_err();

        assert_eq!(error.code, "E_CONFLICT");
        assert_eq!(shared.borrow().transactions, 0);
    }

    #[test]
    fn commit_failure_drops_an_unpublished_xml_spool() {
        let body = "x".repeat(crate::application::value::XML_MEMORY_LIMIT + 1);
        let (mut app, shared) = app(vec![fragment(1, "Big", &body)], true);
        let error = app
            .eval(
                "OUTPUT Big; METADATA Big DESCRIPTION \"changed\";",
                InvocationPolicy::script(),
            )
            .unwrap_err();
        assert_eq!(error.code, "E_COMMIT");
        let path = shared
            .borrow()
            .failed_spool
            .clone()
            .expect("render must have crossed the spill threshold");
        assert!(!path.exists(), "rolled-back value retained its spool");
    }

    #[test]
    fn read_only_outputs_share_one_aggregate_memory_budget() {
        let body = "x".repeat(9 * 1024 * 1024);
        let (mut app, _) = app(
            vec![fragment(1, "First", &body), fragment(2, "Second", &body)],
            false,
        );
        let values = app
            .eval("OUTPUT First; OUTPUT Second;", InvocationPolicy::script())
            .unwrap();
        let Value::Xml(first) = &values[0] else {
            panic!("first result must be XML")
        };
        let Value::Xml(second) = &values[1] else {
            panic!("second result must be XML")
        };
        assert!(first.spilled_path().is_none());
        assert!(second.spilled_path().is_some());
        assert!(first.len() < crate::application::value::XML_MEMORY_LIMIT);
        assert!(second.len() < crate::application::value::XML_MEMORY_LIMIT);
        assert!(first.len() + second.len() > crate::application::value::XML_MEMORY_LIMIT);
    }

    #[test]
    fn aggregate_spool_is_removed_when_transaction_commit_fails() {
        let body = "x".repeat(9 * 1024 * 1024);
        let (mut app, shared) = app(
            vec![fragment(1, "First", &body), fragment(2, "Second", &body)],
            true,
        );
        let error = app
            .eval(
                concat!(
                    "OUTPUT First; OUTPUT Second; ",
                    "METADATA First DESCRIPTION \"changed\";"
                ),
                InvocationPolicy::script(),
            )
            .unwrap_err();
        assert_eq!(error.code, "E_COMMIT");
        let path = shared
            .borrow()
            .failed_spool
            .clone()
            .expect("the second individually-small output must consume the aggregate spill");
        assert!(!path.exists(), "rolled-back aggregate spool was retained");
    }

    #[test]
    fn repeated_fragment_preparation_uses_the_preceding_draft() {
        let snapshot = CatalogSnapshot::new([fragment(1, "Leaf", "durable")]).unwrap();
        let ast = parse_complete("FRAGMENT Leaf; FRAGMENT Leaf;").unwrap();
        let checked =
            crate::language::compile(&ast, &snapshot, InvocationPolicy::interactive()).unwrap();
        let mut provider = SequenceProvider {
            originals: Vec::new(),
            replacements: ["first".to_owned(), "second".to_owned()].into(),
        };
        let prepared = prepare_fragments(&checked, &snapshot, Some(&mut provider))
            .unwrap()
            .unwrap();
        assert_eq!(provider.originals, ["durable", "first"]);
        assert_eq!(
            prepared
                .iter()
                .map(|edit| edit.text.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert!(
            prepared
                .iter()
                .all(|edit| edit.edit_target.base_revision == Some(Revision::new(1).unwrap()))
        );
    }

    #[test]
    fn preparation_identity_follows_delete_and_rename_overlay() {
        for source in [
            "DELETE Leaf; FRAGMENT Leaf;",
            "RENAME Leaf TO Renamed; FRAGMENT Leaf;",
        ] {
            let snapshot = CatalogSnapshot::new([fragment(1, "Leaf", "durable")]).unwrap();
            let ast = parse_complete(source).unwrap();
            let mut checked =
                crate::language::compile(&ast, &snapshot, InvocationPolicy::interactive()).unwrap();
            let mut provider = SequenceProvider {
                originals: Vec::new(),
                replacements: ["replacement".to_owned()].into(),
            };
            let prepared = prepare_fragments(&checked, &snapshot, Some(&mut provider))
                .unwrap()
                .unwrap();
            assert_eq!(provider.originals, [""]);
            inject_prepared(&mut checked, &prepared, &snapshot).unwrap();
        }
    }

    #[test]
    fn prepared_existing_fragment_rejects_authoritative_revision_change() {
        let advisory = CatalogSnapshot::new([fragment(1, "Leaf", "old")]).unwrap();
        let ast = parse_complete("FRAGMENT Leaf;").unwrap();
        let mut checked =
            crate::language::compile(&ast, &advisory, InvocationPolicy::interactive()).unwrap();
        let mut provider = SequenceProvider {
            originals: Vec::new(),
            replacements: ["draft".to_owned()].into(),
        };
        let prepared = prepare_fragments(&checked, &advisory, Some(&mut provider))
            .unwrap()
            .unwrap();
        let mut changed = fragment(1, "Leaf", "concurrent");
        changed.header.revision = Revision::new(2).unwrap();
        let authoritative = CatalogSnapshot::new([changed]).unwrap();
        assert_eq!(
            inject_prepared(&mut checked, &prepared, &authoritative)
                .unwrap_err()
                .code,
            "E_CONFLICT"
        );
    }

    #[test]
    fn provider_may_return_stale_revision_for_same_node_and_gets_conflict() {
        let mut leaf = fragment(1, "Leaf", "current");
        leaf.header.revision = Revision::new(2).unwrap();
        let (mut app, shared) = app(vec![leaf], false);
        let mut provider = StaleIdentityProvider {
            revision: Revision::new(1).unwrap(),
        };

        let error = app
            .eval_with_provider(
                "FRAGMENT Leaf;",
                InvocationPolicy::interactive(),
                &mut provider,
            )
            .unwrap_err();

        assert_eq!(error.code, "E_CONFLICT");
        assert_eq!(shared.borrow().nodes[0].header.revision.get(), 2);
    }

    #[test]
    fn find_saturates_path_count_instead_of_failing() {
        let mut nodes = vec![fragment(1, "Needle", "match")];
        for id in 2..=66 {
            nodes.push(prompt(
                id,
                &format!("P{id}"),
                vec![NodeId::new(id - 1).unwrap(); 2],
            ));
        }
        let snapshot = CatalogSnapshot::new(nodes).unwrap();
        let hits = search(
            &snapshot,
            "Needle",
            SearchField::Title,
            Some(&Symbol::new("P66").unwrap()),
            SearchMatcher::Fuzzy,
        )
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].occurrence_count, u64::MAX);
    }

    #[test]
    fn invalid_parse_preserves_frontend_code() {
        let error = parse_complete("@").unwrap_err();
        assert_eq!(error.code, "E0001");
    }

    #[test]
    fn commit_failure_discards_metadata_and_buffered_output() {
        let (mut app, shared) = app(vec![fragment(1, "Leaf", "body")], true);
        let error = app
            .eval(
                "METADATA Leaf DESCRIPTION \"changed\"; OUTPUT Leaf;",
                InvocationPolicy::script(),
            )
            .unwrap_err();
        assert_eq!(error.code, "E_COMMIT");
        let state = shared.borrow();
        assert_eq!(state.nodes[0].header.metadata.description(), None);
        assert_eq!(state.nodes[0].header.revision.get(), 1);
        assert_eq!(state.transactions, 1);
    }
}
