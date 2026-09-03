//! 编译、准备、事务和解释的协调器。 / Coordinator for compile, prepare, transaction, and interpretation.

use super::{InvocationPolicy, Promptr, Value, ports::CatalogWrite};
use crate::{
    application::{CheckedProgram, NodeView, Op, command::NodeFilter},
    diagnostic::{Diagnostic, DiagnosticCategory, Result, SourceSpan},
    domain::{
        CatalogSnapshot, Metadata, Node, NodeBody, NodeKind, SearchField, SearchHit, Symbol, Tag,
        XmlText,
    },
    infrastructure::editor::{EditRequest, EditTarget, EditorOutcome, TextProvider},
    language::{ParseOutcome, Program, parse},
};
use nucleo_matcher::{Config as MatcherConfig, Matcher, Utf32Str};
use std::collections::BTreeMap;

#[derive(Clone)]
struct PreparedFragment {
    target: Symbol,
    edit_target: EditTarget,
    text: XmlText,
}

#[derive(Clone)]
struct StagedFragment {
    target: EditTarget,
    text: String,
}

/// @brief 仅解析并按当前快照检查源码。 / Parse and check source against the current snapshot only.
/// @param app 应用门面。 / Application facade.
/// @param source DSL 源码。 / DSL source.
/// @param policy 调用能力策略。 / Invocation capability policy.
/// @return 已检查程序。 / Checked program.
pub(crate) fn check(
    app: &Promptr,
    source: &str,
    policy: InvocationPolicy,
) -> Result<CheckedProgram> {
    let ast = parse_complete(source)?;
    let snapshot = app.database.snapshot()?;
    crate::language::compile(&ast, &snapshot, policy)
}

/// @brief 通过共享运行时执行一段源码。 / Execute source through the shared runtime.
/// @param app 应用门面。 / Application facade.
/// @param source DSL 源码。 / DSL source.
/// @param policy 调用能力策略。 / Invocation capability policy.
/// @param provider 可选交互文本提供者。 / Optional interactive text provider.
/// @return 提交后可发布的类型化值。 / Typed values publishable after commit.
pub(crate) fn eval(
    app: &mut Promptr,
    source: &str,
    policy: InvocationPolicy,
    provider: Option<&mut dyn TextProvider>,
) -> Result<Vec<Value>> {
    let ast = parse_complete(source)?;
    let advisory_snapshot = app.database.snapshot()?;
    let checked = crate::language::compile(&ast, &advisory_snapshot, policy)?;
    let Some(prepared) = prepare_fragments(&checked, &advisory_snapshot, provider)? else {
        return Ok(Vec::new());
    };
    if !checked.is_mutating() {
        return interpret_read_only(&checked, &advisory_snapshot);
    }
    let mut operation = |transaction: &mut dyn CatalogWrite| {
        let authoritative = transaction.snapshot()?;
        let mut program = crate::language::compile(&ast, &authoritative, policy)?;
        inject_prepared(&mut program, &prepared, &authoritative)?;
        interpret_transaction(&program, transaction)
    };
    app.database.write_transaction(&mut operation)
}

/// @brief 把解析器状态稳定映射为公共诊断。 / Map parser states to stable public diagnostics.
fn parse_complete(source: &str) -> Result<Program> {
    let convert = |code: &'static str, parsed: crate::language::ParseDiagnostic| {
        Diagnostic::error(code, DiagnosticCategory::Syntax, parsed.message)
            .with_span(SourceSpan::new(parsed.span.start, parsed.span.end))
            .with_cause(parsed.code.as_str())
    };
    match parse(source) {
        ParseOutcome::Complete(program) => Ok(program),
        ParseOutcome::Incomplete(parsed) => {
            Err(convert("E_INCOMPLETE", parsed)
                .with_hint("append the missing DSL input and try again"))
        }
        ParseOutcome::Invalid(parsed) => Err(convert("E_SYNTAX", parsed)),
    }
}

/// @brief 写锁外准备所有片段文本。 / Prepare all fragment text outside the writer lock.
/// @return `None` 表示用户取消整个调用。 / `None` means the user cancelled the whole invocation.
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
                if edit.target != request.target {
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
                        target: request.target,
                        text: text.as_str().to_owned(),
                    },
                );
                prepared.push(PreparedFragment {
                    target: target.clone(),
                    edit_target: request.target,
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

/// @brief 将锁外准备的正文注入权威重编译结果。 / Inject prepared bodies into the authoritative recompilation.
fn inject_prepared(
    program: &mut CheckedProgram,
    prepared: &[PreparedFragment],
    authoritative: &CatalogSnapshot,
) -> Result<()> {
    let mut edits = prepared.iter();
    for op in &mut program.ops {
        if let Op::UpsertFragment {
            text,
            expected_revision,
            target,
        } = op
        {
            let edit = edits.next().ok_or_else(internal_program_mismatch)?;
            if edit.target != *target {
                return Err(internal_program_mismatch());
            }
            // New-node races are also conflicts: advisory absence must still be
            // authoritative absence after the writer reservation is acquired.
            let identity_matches = match (edit.edit_target.node_id, edit.edit_target.base_revision)
            {
                (Some(id), Some(revision)) => authoritative
                    .get(id)
                    .is_some_and(|node| node.header.revision == revision),
                (None, None) => authoritative.get_by_symbol(&edit.target).is_none(),
                _ => false,
            };
            if !identity_matches {
                return Err(Diagnostic::error(
                    "E_CONFLICT",
                    DiagnosticCategory::Conflict,
                    format!("fragment `{target}` changed while it was being edited"),
                ));
            }
            *text = Some(edit.text.clone());
            *expected_revision = edit.edit_target.base_revision;
        }
    }
    if edits.next().is_some() {
        return Err(internal_program_mismatch());
    }
    Ok(())
}

/// @brief 在一个不可变快照上执行纯查询。 / Execute pure queries on one immutable snapshot.
fn interpret_read_only(program: &CheckedProgram, snapshot: &CatalogSnapshot) -> Result<Vec<Value>> {
    program
        .ops
        .iter()
        .map(|op| interpret_query(op, snapshot))
        .collect()
}

/// @brief 在事务内按源码顺序执行并缓冲结果。 / Execute in source order and buffer results inside a transaction.
fn interpret_transaction(
    program: &CheckedProgram,
    transaction: &mut dyn CatalogWrite,
) -> Result<Vec<Value>> {
    let mut values = Vec::with_capacity(program.ops.len());
    for op in &program.ops {
        let value = match op {
            Op::UpsertFragment {
                target,
                text,
                expected_revision,
            } => {
                let current_revision = transaction
                    .snapshot()?
                    .get_by_symbol(target)
                    .map(|node| node.header.revision);
                // The coordinator already compared the prepared base with the
                // authoritative pre-mutation snapshot. Here the current revision
                // may legitimately include earlier operations in this same script.
                let _ = expected_revision;
                transaction.upsert_fragment(
                    target,
                    text.as_ref().ok_or_else(internal_program_mismatch)?,
                    current_revision,
                )?;
                Value::Unit
            }
            Op::ReplacePrompt { target, children } => {
                transaction.replace_prompt(target, children)?;
                Value::Unit
            }
            Op::Rename { target, new_symbol } => {
                transaction.rename(target, new_symbol)?;
                Value::Unit
            }
            Op::Delete { target } => {
                transaction.delete(target)?;
                Value::Unit
            }
            Op::SetDescription {
                target,
                description,
            } => {
                let mut metadata = current_metadata(transaction, target)?;
                metadata.set_description(description.clone());
                transaction.set_metadata(target, &metadata)?;
                Value::Metadata(metadata)
            }
            Op::SetTags { target, tags } => {
                let parsed = tags
                    .iter()
                    .cloned()
                    .map(Tag::new)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(domain_diagnostic)?;
                let mut metadata = current_metadata(transaction, target)?;
                metadata.set_tags(parsed);
                transaction.set_metadata(target, &metadata)?;
                Value::Metadata(metadata)
            }
            query => {
                let snapshot = transaction.snapshot()?;
                interpret_query(query, &snapshot)?
            }
        };
        values.push(value);
    }
    Ok(values)
}

fn current_metadata(transaction: &dyn CatalogWrite, target: &Symbol) -> Result<Metadata> {
    transaction
        .snapshot()?
        .get_by_symbol(target)
        .map(|node| node.header.metadata.clone())
        .ok_or_else(|| unknown_symbol(target))
}

/// @brief 解释一个无副作用操作。 / Interpret one side-effect-free operation.
fn interpret_query(op: &Op, snapshot: &CatalogSnapshot) -> Result<Value> {
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
            *field,
            root.as_ref(),
        )?)),
        Op::RenderXml { root } => {
            let node = snapshot
                .get_by_symbol(root)
                .ok_or_else(|| unknown_symbol(root))?;
            let bytes = snapshot.render_xml_vec(node.header.id).map_err(|error| {
                Diagnostic::error(
                    "E_RENDER",
                    DiagnosticCategory::Domain,
                    "canonical XML rendering failed",
                )
                .with_cause(error.to_string())
            })?;
            Ok(Value::Xml(String::from_utf8(bytes).map_err(|error| {
                Diagnostic::error(
                    "E_INTERNAL_UTF8",
                    DiagnosticCategory::Internal,
                    "renderer produced non-UTF-8 bytes",
                )
                .with_cause(error.to_string())
            })?))
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
            .occurrence_counts(root.header.id)
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
            node.header.symbol.as_str(),
            text.as_str(),
            field,
        ) {
            hits.push(SearchHit {
                node_id: node.header.id,
                symbol: node.header.symbol.clone(),
                score: f64::from(score),
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
    title: &str,
    content: &str,
    field: SearchField,
) -> Option<u16> {
    fn one(matcher: &mut Matcher, needle: Utf32Str<'_>, value: &str) -> Option<u16> {
        let mut buffer = Vec::new();
        matcher.fuzzy_match(Utf32Str::new(value, &mut buffer), needle)
    }
    match field {
        SearchField::Title => one(matcher, needle, title),
        SearchField::Content => one(matcher, needle, content),
        SearchField::Mixed => match (one(matcher, needle, title), one(matcher, needle, content)) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
        },
    }
}

fn fragment_text(node: &Node) -> Option<&str> {
    match &node.body {
        NodeBody::Fragment(text) => Some(text.as_str()),
        NodeBody::Prompt(_) => None,
    }
}
fn domain_diagnostic(error: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::error(
        "E_DOMAIN",
        DiagnosticCategory::Domain,
        "domain validation failed",
    )
    .with_cause(error.to_string())
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
            self.0.borrow_mut().transactions += 1;
            let mut writer = FakeWrite {
                nodes: self.0.borrow().nodes.clone(),
            };
            let values = operation(&mut writer)?;
            if self.0.borrow().fail_commit {
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
