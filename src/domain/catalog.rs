use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::{self, Write},
};

use super::{DomainError, Node, NodeBody, NodeId, Symbol};

/// @brief 规范 XML 渲染失败。 / Canonical XML rendering failure.
#[derive(Debug)]
pub enum RenderError {
    /// @brief 快照违反领域不变量。 / Snapshot violates a domain invariant.
    Domain(DomainError),
    /// @brief 输出汇写入失败。 / Output sink write failure.
    Io(io::Error),
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Domain(error) => Some(error),
            Self::Io(error) => Some(error),
        }
    }
}

impl From<DomainError> for RenderError {
    fn from(value: DomainError) -> Self {
        Self::Domain(value)
    }
}

impl From<io::Error> for RenderError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// @brief 已完整校验的不可变内存目录快照。 / Fully validated immutable in-memory catalog snapshot.
#[derive(Debug, Clone, Default)]
pub struct CatalogSnapshot {
    /// @brief 按稳定内部标识索引的节点。 / Nodes indexed by stable internal identity.
    nodes: BTreeMap<NodeId, Node>,
    /// @brief 全局唯一的符号绑定。 / Globally unique symbol bindings.
    symbols: BTreeMap<Symbol, NodeId>,
}

impl CatalogSnapshot {
    /// @brief 建立快照并验证唯一性、引用完整性和无环性。 / Build a snapshot and validate uniqueness, references, and acyclicity.
    /// @param nodes 无顺序要求的节点集合。 / Node collection with no ordering requirement.
    /// @return 有效快照或领域错误。 / Valid snapshot or domain error.
    pub fn new(nodes: impl IntoIterator<Item = Node>) -> Result<Self, DomainError> {
        let mut snapshot = Self::default();
        for node in nodes {
            // 在聚合边界再次声明该持久化不变量，避免未来新增构造路径时静默放宽。
            // Reassert the persistence invariant at the aggregate boundary so future
            // construction paths cannot silently weaken it.
            if node.header.id.get() <= 0 {
                return Err(DomainError::InvalidPositiveInteger {
                    kind: "node id",
                    value: i128::from(node.header.id.get()),
                });
            }
            let actual = node.body.kind();
            if node.header.kind != actual {
                return Err(DomainError::KindMismatch {
                    declared: node.header.kind,
                    actual,
                });
            }
            let id = node.header.id;
            let symbol = node.header.symbol.clone();
            if snapshot.nodes.contains_key(&id) {
                return Err(DomainError::DuplicateNodeId(id));
            }
            if snapshot.symbols.insert(symbol.clone(), id).is_some() {
                return Err(DomainError::DuplicateSymbol(symbol.into_inner()));
            }
            snapshot.nodes.insert(id, node);
        }
        snapshot.validate_references()?;
        snapshot.validate_acyclic()?;
        Ok(snapshot)
    }

    /// @brief 返回节点数量。 / Return the node count.
    /// @return 节点数量。 / Node count.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// @brief 判断快照是否为空。 / Test whether the snapshot is empty.
    /// @return 为空时为真。 / True when empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// @brief 按 ID 查询节点。 / Look up a node by id.
    /// @param id 节点 ID。 / Node id.
    /// @return 可选节点引用。 / Optional node reference.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// @brief 按符号查询节点。 / Look up a node by symbol.
    /// @param symbol 节点符号。 / Node symbol.
    /// @return 可选节点引用。 / Optional node reference.
    pub fn get_by_symbol(&self, symbol: &Symbol) -> Option<&Node> {
        self.symbols.get(symbol).and_then(|id| self.nodes.get(id))
    }

    /// @brief 按符号字节序迭代全部节点。 / Iterate all nodes in bytewise symbol order.
    /// @return 节点迭代器。 / Node iterator.
    pub fn iter_by_symbol(&self) -> impl ExactSizeIterator<Item = &Node> {
        self.symbols.values().map(|id| &self.nodes[id])
    }

    /// @brief 迭代计算从起点可达的唯一节点集合。 / Iteratively compute unique nodes reachable from a root.
    /// @param root 起始节点。 / Root node.
    /// @return 包含起点的可达集合。 / Reachable set including the root.
    pub fn reachable(&self, root: NodeId) -> Result<BTreeSet<NodeId>, DomainError> {
        if !self.nodes.contains_key(&root) {
            return Err(DomainError::MissingNode(root));
        }
        let mut seen = BTreeSet::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let NodeBody::Prompt(children) = &self
                .nodes
                .get(&id)
                .ok_or(DomainError::MissingNode(id))?
                .body
            {
                pending.extend(children.as_slice().iter().rev().copied());
            }
        }
        Ok(seen)
    }

    /// @brief 判断目标是否可由起点到达。 / Test whether a target is reachable from a root.
    /// @param root 起点。 / Root.
    /// @param target 目标。 / Target.
    /// @return 可达性。 / Reachability.
    pub fn is_reachable(&self, root: NodeId, target: NodeId) -> Result<bool, DomainError> {
        Ok(self.reachable(root)?.contains(&target))
    }

    /// @brief 校验替换 Prompt 子列表不会引入悬空引用或环。 / Validate that replacing prompt children introduces neither dangling references nor cycles.
    /// @param parent 被替换的 Prompt。 / Prompt being replaced.
    /// @param children 候选子列表，可保留重复项。 / Candidate children, with duplicates preserved.
    /// @return 校验成功或领域错误。 / Success or domain error.
    /// @note 该函数不修改快照。 / This function does not mutate the snapshot.
    pub fn validate_replacement(
        &self,
        parent: NodeId,
        children: &[NodeId],
    ) -> Result<(), DomainError> {
        if !self.nodes.contains_key(&parent) {
            return Err(DomainError::MissingNode(parent));
        }
        if children.is_empty() {
            return Err(DomainError::EmptyChildren);
        }
        for &child in children {
            if !self.nodes.contains_key(&child) {
                return Err(DomainError::MissingNode(child));
            }
            if child == parent || self.is_reachable(child, parent)? {
                return Err(DomainError::CycleDetected { at: parent });
            }
        }
        Ok(())
    }

    /// @brief 计算完全展开树中每个节点的路径出现次数。 / Count path occurrences of every node in the fully expanded tree.
    /// @param root 展开根。 / Expansion root.
    /// @return 仅含可达节点的计数，根计为一次。 / Counts for reachable nodes only, with root counted once.
    pub fn occurrence_counts(&self, root: NodeId) -> Result<BTreeMap<NodeId, u64>, DomainError> {
        let reachable = self.reachable(root)?;
        let mut incoming: BTreeMap<NodeId, usize> = reachable.iter().map(|&id| (id, 0)).collect();
        for &id in &reachable {
            if let NodeBody::Prompt(children) = &self.nodes[&id].body {
                for child in children.as_slice() {
                    let degree = incoming
                        .get_mut(child)
                        .ok_or(DomainError::MissingNode(*child))?;
                    *degree = degree
                        .checked_add(1)
                        .ok_or(DomainError::OccurrenceOverflow { at: *child })?;
                }
            }
        }

        let mut counts: BTreeMap<NodeId, u64> = reachable.iter().map(|&id| (id, 0)).collect();
        counts.insert(root, 1);
        let mut ready: Vec<NodeId> = incoming
            .iter()
            .filter_map(|(&id, &degree)| (degree == 0).then_some(id))
            .collect();
        let mut visited = 0usize;
        while let Some(id) = ready.pop() {
            visited += 1;
            let parent_count = counts[&id];
            if let NodeBody::Prompt(children) = &self.nodes[&id].body {
                for &child in children.as_slice() {
                    let count = counts
                        .get_mut(&child)
                        .ok_or(DomainError::MissingNode(child))?;
                    *count = count
                        .checked_add(parent_count)
                        .ok_or(DomainError::OccurrenceOverflow { at: child })?;
                    let degree = incoming
                        .get_mut(&child)
                        .ok_or(DomainError::MissingNode(child))?;
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push(child);
                    }
                }
            }
        }
        if visited != reachable.len() {
            let at = incoming
                .into_iter()
                .find_map(|(id, degree)| (degree != 0).then_some(id))
                .unwrap_or(root);
            return Err(DomainError::CycleDetected { at });
        }
        Ok(counts)
    }

    /// @brief 以饱和算术计算完全展开树中的路径出现次数。 / Count expanded-tree path occurrences with saturating arithmetic.
    /// @param root 展开根。 / Expansion root.
    /// @return 可达节点计数；超过 `u64::MAX` 的值固定为该上限。 / Reachable-node counts, clamped to `u64::MAX` on overflow.
    /// @note 该查询不会因出现次数溢出而失败；结构损坏仍返回领域错误。 / Occurrence overflow never fails this query; structural corruption still returns a domain error.
    pub fn occurrence_counts_saturating(
        &self,
        root: NodeId,
    ) -> Result<BTreeMap<NodeId, u64>, DomainError> {
        let reachable = self.reachable(root)?;
        let mut incoming: BTreeMap<NodeId, usize> = reachable.iter().map(|&id| (id, 0)).collect();
        for &id in &reachable {
            if let NodeBody::Prompt(children) = &self.nodes[&id].body {
                for child in children.as_slice() {
                    let degree = incoming
                        .get_mut(child)
                        .ok_or(DomainError::MissingNode(*child))?;
                    *degree = degree.saturating_add(1);
                }
            }
        }

        let mut counts: BTreeMap<NodeId, u64> = reachable.iter().map(|&id| (id, 0)).collect();
        counts.insert(root, 1);
        let mut ready: Vec<NodeId> = incoming
            .iter()
            .filter_map(|(&id, &degree)| (degree == 0).then_some(id))
            .collect();
        let mut visited = 0usize;
        while let Some(id) = ready.pop() {
            visited += 1;
            let parent_count = counts[&id];
            if let NodeBody::Prompt(children) = &self.nodes[&id].body {
                for &child in children.as_slice() {
                    let count = counts
                        .get_mut(&child)
                        .ok_or(DomainError::MissingNode(child))?;
                    *count = count.saturating_add(parent_count);
                    let degree = incoming
                        .get_mut(&child)
                        .ok_or(DomainError::MissingNode(child))?;
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push(child);
                    }
                }
            }
        }
        if visited != reachable.len() {
            let at = incoming
                .into_iter()
                .find_map(|(id, degree)| (degree != 0).then_some(id))
                .unwrap_or(root);
            return Err(DomainError::CycleDetected { at });
        }
        Ok(counts)
    }

    /// @brief 以迭代算法把规范 XML 写入输出汇。 / Iteratively write canonical XML to a sink.
    /// @param root 根节点。 / Root node.
    /// @param sink 字节输出汇。 / Byte sink.
    /// @return 成功或渲染错误。 / Success or rendering error.
    /// @note 输出仅转义 `&<>`，并在根元素后恰好追加一个 LF。 / Output escapes only `&<>` and appends exactly one LF after the root element.
    pub fn render_xml<W: Write>(&self, root: NodeId, sink: &mut W) -> Result<(), RenderError> {
        enum Task {
            Visit(NodeId),
            Close(NodeId),
        }
        if !self.nodes.contains_key(&root) {
            return Err(DomainError::MissingNode(root).into());
        }
        let mut pending = vec![Task::Visit(root)];
        while let Some(task) = pending.pop() {
            match task {
                Task::Visit(id) => {
                    let node = self.nodes.get(&id).ok_or(DomainError::MissingNode(id))?;
                    write!(sink, "<{}>", node.header.symbol.as_str())?;
                    match &node.body {
                        NodeBody::Fragment(text) => {
                            write_escaped(text.as_str(), sink)?;
                            write!(sink, "</{}>", node.header.symbol.as_str())?;
                        }
                        NodeBody::Prompt(children) => {
                            pending.push(Task::Close(id));
                            pending
                                .extend(children.as_slice().iter().rev().copied().map(Task::Visit));
                        }
                    }
                }
                Task::Close(id) => {
                    let node = self.nodes.get(&id).ok_or(DomainError::MissingNode(id))?;
                    write!(sink, "</{}>", node.header.symbol.as_str())?;
                }
            }
        }
        sink.write_all(b"\n")?;
        Ok(())
    }

    /// @brief 返回规范 XML 字节。 / Return canonical XML bytes.
    /// @param root 根节点。 / Root node.
    /// @return UTF-8 XML 字节或渲染错误。 / UTF-8 XML bytes or rendering error.
    pub fn render_xml_vec(&self, root: NodeId) -> Result<Vec<u8>, RenderError> {
        let mut output = Vec::new();
        self.render_xml(root, &mut output)?;
        Ok(output)
    }

    fn validate_references(&self) -> Result<(), DomainError> {
        for node in self.nodes.values() {
            if let NodeBody::Prompt(children) = &node.body {
                for child in children.as_slice() {
                    if !self.nodes.contains_key(child) {
                        return Err(DomainError::MissingNode(*child));
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_acyclic(&self) -> Result<(), DomainError> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Color {
            White,
            Gray,
            Black,
        }
        let mut colors: BTreeMap<NodeId, Color> =
            self.nodes.keys().map(|&id| (id, Color::White)).collect();
        for &start in self.nodes.keys() {
            if colors[&start] != Color::White {
                continue;
            }
            colors.insert(start, Color::Gray);
            let mut stack = vec![(start, 0usize)];
            while let Some((id, next_child)) = stack.last_mut() {
                let children = match &self.nodes[id].body {
                    NodeBody::Fragment(_) => &[][..],
                    NodeBody::Prompt(children) => children.as_slice(),
                };
                if *next_child == children.len() {
                    colors.insert(*id, Color::Black);
                    stack.pop();
                    continue;
                }
                let child = children[*next_child];
                *next_child += 1;
                match colors
                    .get(&child)
                    .copied()
                    .ok_or(DomainError::MissingNode(child))?
                {
                    Color::Gray => return Err(DomainError::CycleDetected { at: child }),
                    Color::Black => {}
                    Color::White => {
                        colors.insert(child, Color::Gray);
                        stack.push((child, 0));
                    }
                }
            }
        }
        Ok(())
    }
}

fn write_escaped<W: Write>(text: &str, sink: &mut W) -> io::Result<()> {
    let mut segment_start = 0;
    for (offset, byte) in text.bytes().enumerate() {
        let escape: &[u8] = match byte {
            b'&' => b"&amp;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            _ => continue,
        };
        sink.write_all(&text.as_bytes()[segment_start..offset])?;
        sink.write_all(escape)?;
        segment_start = offset + 1;
    }
    sink.write_all(&text.as_bytes()[segment_start..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Metadata, NodeHeader, NonEmptyChildren, Revision, XmlText};
    use proptest::prelude::*;

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
    fn fragment(value: i64, symbol: &str, text: &str) -> Node {
        node(
            value,
            symbol,
            NodeBody::Fragment(XmlText::new(text).unwrap()),
        )
    }
    fn prompt(value: i64, symbol: &str, children: Vec<NodeId>) -> Node {
        node(
            value,
            symbol,
            NodeBody::Prompt(NonEmptyChildren::new(children).unwrap()),
        )
    }

    #[test]
    fn canonical_render_preserves_duplicate_occurrences_and_order() {
        let snapshot = CatalogSnapshot::new([
            fragment(1, "A", "<&>\"'"),
            fragment(2, "B", "b"),
            prompt(3, "Root", vec![id(2), id(1), id(1)]),
        ])
        .unwrap();
        assert_eq!(
            snapshot.render_xml_vec(id(3)).unwrap(),
            b"<Root><B>b</B><A>&lt;&amp;&gt;\"'</A><A>&lt;&amp;&gt;\"'</A></Root>\n"
        );
        assert_eq!(snapshot.occurrence_counts(id(3)).unwrap()[&id(1)], 2);
    }

    #[test]
    fn shared_subgraph_counts_expanded_paths() {
        let snapshot = CatalogSnapshot::new([
            fragment(1, "Leaf", "x"),
            prompt(2, "Shared", vec![id(1), id(1)]),
            prompt(3, "Root", vec![id(2), id(2)]),
        ])
        .unwrap();
        let counts = snapshot.occurrence_counts(id(3)).unwrap();
        assert_eq!(counts[&id(2)], 2);
        assert_eq!(counts[&id(1)], 4);
    }

    #[test]
    fn rejects_cycles_and_dangling_edges() {
        assert!(matches!(
            CatalogSnapshot::new([prompt(1, "A", vec![id(1)])]),
            Err(DomainError::CycleDetected { .. })
        ));
        assert_eq!(
            CatalogSnapshot::new([prompt(1, "A", vec![id(9)])]).unwrap_err(),
            DomainError::MissingNode(id(9))
        );
    }

    #[test]
    fn deep_graph_algorithms_do_not_recurse() {
        const DEPTH: i64 = 20_000;
        let mut nodes = Vec::with_capacity(DEPTH as usize);
        nodes.push(fragment(DEPTH, &format!("N{DEPTH}"), "x"));
        for value in (1..DEPTH).rev() {
            nodes.push(prompt(value, &format!("N{value}"), vec![id(value + 1)]));
        }
        let snapshot = CatalogSnapshot::new(nodes).unwrap();
        assert_eq!(snapshot.reachable(id(1)).unwrap().len(), DEPTH as usize);
        assert_eq!(snapshot.occurrence_counts(id(1)).unwrap()[&id(DEPTH)], 1);
        let rendered = snapshot.render_xml_vec(id(1)).unwrap();
        assert!(rendered.ends_with(b"</N1>\n"));
    }

    #[test]
    fn replacement_detects_indirect_cycle_without_mutating() {
        let snapshot = CatalogSnapshot::new([
            fragment(1, "Leaf", ""),
            prompt(2, "Inner", vec![id(1)]),
            prompt(3, "Root", vec![id(2)]),
        ])
        .unwrap();
        assert!(matches!(
            snapshot.validate_replacement(id(2), &[id(3)]),
            Err(DomainError::CycleDetected { .. })
        ));
        assert!(
            snapshot
                .validate_replacement(id(3), &[id(1), id(1)])
                .is_ok()
        );
    }

    #[test]
    fn saturating_occurrences_remain_queryable_after_overflow() {
        let leaf_id = id(66);
        let mut nodes = vec![fragment(66, "N66", "x")];
        for value in (1..66).rev() {
            nodes.push(prompt(
                value,
                &format!("N{value}"),
                vec![id(value + 1), id(value + 1)],
            ));
        }
        let snapshot = CatalogSnapshot::new(nodes).unwrap();
        assert!(matches!(
            snapshot.occurrence_counts(id(1)),
            Err(DomainError::OccurrenceOverflow { .. })
        ));
        assert_eq!(
            snapshot.occurrence_counts_saturating(id(1)).unwrap()[&leaf_id],
            u64::MAX
        );
    }

    proptest! {
        #[test]
        fn duplicate_edges_preserve_generated_multiplicity(multiplicity in 1usize..256) {
            let snapshot = CatalogSnapshot::new([
                fragment(1, "Leaf", "&"),
                prompt(2, "Root", vec![id(1); multiplicity]),
            ]).unwrap();
            prop_assert_eq!(snapshot.occurrence_counts(id(2)).unwrap()[&id(1)], multiplicity as u64);
            let output = snapshot.render_xml_vec(id(2)).unwrap();
            prop_assert_eq!(output.iter().filter(|&&byte| byte == b'\n').count(), 1);
            prop_assert!(output.ends_with(b"</Root>\n"));
        }
    }
}
