// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Fallible, release-owned indexes for the compiled schema. No deletion is needed.
use super::reference::work;
use crate::contract::{ScratchCharge, format_output_governed};
use crate::{Governor, ScratchString, ScratchVec, StopReason};
use core::{
    cmp::Ordering,
    ops::{Index, IndexMut},
};

pub(super) trait Key: ScratchCharge {
    fn compare(&self, other: &Self) -> Ordering;
    fn units(&self) -> usize;
}
pub(super) trait Query<K> {
    fn compare(&self, stored: &K) -> Ordering;
    fn units(&self) -> usize;
}
impl<K: Key> Query<K> for K {
    fn compare(&self, stored: &K) -> Ordering {
        Key::compare(self, stored)
    }
    fn units(&self) -> usize {
        Key::units(self)
    }
}
impl Key for ScratchString {
    fn compare(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
    fn units(&self) -> usize {
        self.len()
    }
}
impl Query<ScratchString> for str {
    fn compare(&self, stored: &ScratchString) -> Ordering {
        self.cmp(stored.as_str())
    }
    fn units(&self) -> usize {
        self.len()
    }
}
impl Query<ScratchString> for String {
    fn compare(&self, stored: &ScratchString) -> Ordering {
        self.as_str().cmp(stored.as_str())
    }
    fn units(&self) -> usize {
        self.len()
    }
}
impl Key for usize {
    fn compare(&self, other: &Self) -> Ordering {
        self.cmp(other)
    }
    fn units(&self) -> usize {
        1
    }
}
#[derive(Debug)]
pub(super) struct Route {
    pub(super) document: usize,
    pub(super) positions: ScratchVec<usize>,
}
impl ScratchCharge for Route {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 48);
        48
    };
}
impl Key for Route {
    fn compare(&self, other: &Self) -> Ordering {
        self.document
            .cmp(&other.document)
            .then_with(|| self.positions.as_slice().cmp(other.positions.as_slice()))
    }
    fn units(&self) -> usize {
        self.positions.len().saturating_add(1)
    }
}
impl Query<Route> for (usize, &[usize]) {
    fn compare(&self, stored: &Route) -> Ordering {
        self.0
            .cmp(&stored.document)
            .then_with(|| self.1.cmp(stored.positions.as_slice()))
    }
    fn units(&self) -> usize {
        self.1.len().saturating_add(1)
    }
}
#[derive(Debug)]
struct Node<K, V> {
    key: K,
    value: V,
    parent: Option<usize>,
    left: Option<usize>,
    right: Option<usize>,
    height: u32,
}
impl<K: ScratchCharge, V: ScratchCharge> ScratchCharge for Node<K, V> {
    // Three portable Option<usize> slots (48), height and alignment (8).
    const CHARGE: usize = {
        let bytes = K::CHARGE + V::CHARGE + 56;
        assert!(size_of::<Self>() <= bytes);
        bytes
    };
}
#[derive(Debug)]
pub(super) struct Map<K, V> {
    nodes: ScratchVec<Node<K, V>>,
    root: Option<usize>,
}
impl<K, V> ScratchCharge for Map<K, V> {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 56);
        56
    };
}
impl<K: Key, V: ScratchCharge> Map<K, V> {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            nodes: ScratchVec::new(governor),
            root: None,
        }
    }
    /// Consume strictly increasing keys and link their balanced tree once.
    /// Invalid ordering is an invariant failure, never an insertion fallback.
    pub(super) fn from_sorted(
        entries: ScratchVec<(K, V)>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        if !Self::strictly_ordered(&entries, governor)? {
            return Err(StopReason::InvalidInput(format_output_governed(
                format_args!("sorted schema index requires strictly increasing unique keys"),
                governor,
            )?));
        }
        Self::from_ordered(entries, governor)
    }

    fn strictly_ordered(entries: &[(K, V)], governor: &mut Governor) -> Result<bool, StopReason> {
        governor.check()?;
        for pair in entries.windows(2) {
            work(governor, Key::units(&pair[0].0).saturating_add(1))?;
            if Key::compare(&pair[0].0, &pair[1].0) != Ordering::Less {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Both callers have established strict ordering with admitted comparisons.
    fn from_ordered(
        mut entries: ScratchVec<(K, V)>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let mut map = Self::new(governor);
        map.nodes.reserve(entries.len())?;
        // Pop transfers ownership without a permanently charged into_vec or
        // a second reversal. Arena indices therefore run in descending order.
        while !entries.is_empty() {
            // Pop/push and the six initialized node fields.
            work(governor, 8)?;
            let (key, value) = entries.pop().expect("nonempty sorted entries");
            map.nodes.push(Node {
                key,
                value,
                parent: None,
                left: None,
                right: None,
                height: 1,
            })?;
        }
        let (root, _) = map.link_sorted(0, map.nodes.len(), None, governor)?;
        work(governor, 1)?;
        map.root = root;
        Ok(map)
    }

    /// Assemble unordered entries, keeping the last value for each equal key.
    pub(super) fn from_entries(
        mut entries: ScratchVec<(K, V)>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        governor.check()?;
        let mut map = Self::new(governor);
        // Ordinary insertion avoids the ordering scan and recursive linkage
        // for empty or singleton maps.
        if entries.len() <= 1 {
            work(governor, 1)?;
            if let Some((key, value)) = entries.pop() {
                map.insert(key, value, governor)?;
            }
            return Ok(map);
        }
        if Self::strictly_ordered(&entries, governor)? {
            return Self::from_ordered(entries, governor);
        }
        // Preserve original insertion order and last-write-wins semantics.
        // Reversing once lets pop transfer owners without permanent publication
        // or a second buffer. Unordered inputs use the normal AVL algorithm.
        work(governor, (entries.len() / 2).saturating_mul(3))?;
        entries.as_mut_slice().reverse();
        while !entries.is_empty() {
            work(governor, 1)?;
            let (key, value) = entries.pop().expect("nonempty unordered entries");
            map.insert(key, value, governor)?;
        }
        Ok(map)
    }

    /// Link a descending arena interval; recursion depth is logarithmic.
    fn link_sorted(
        &mut self,
        start: usize,
        end: usize,
        parent: Option<usize>,
        governor: &mut Governor,
    ) -> Result<(Option<usize>, u32), StopReason> {
        work(governor, 1)?;
        if start == end {
            return Ok((None, 0));
        }
        // Midpoint/bounds arithmetic, four field writes, height calculation
        // and returned linkage. Child calls admit their own operations.
        work(governor, 12)?;
        let id = start + (end - start) / 2;
        let (left, left_height) = self.link_sorted(id + 1, end, Some(id), governor)?;
        let (right, right_height) = self.link_sorted(start, id, Some(id), governor)?;
        let height = 1 + left_height.max(right_height);
        let node = &mut self.nodes.as_mut_slice()[id];
        node.parent = parent;
        node.left = left;
        node.right = right;
        node.height = height;
        Ok((Some(id), height))
    }

    fn search<Q: Query<K> + ?Sized>(
        &self,
        query: &Q,
        mut governor: Option<&mut Governor>,
    ) -> Result<Option<usize>, StopReason> {
        let mut cursor = self.root;
        while let Some(id) = cursor {
            if let Some(governor) = governor.as_deref_mut() {
                work(governor, query.units().saturating_add(1))?;
            }
            let node = &self.nodes[id];
            match query.compare(&node.key) {
                Ordering::Equal => return Ok(Some(id)),
                Ordering::Less => cursor = node.left,
                Ordering::Greater => cursor = node.right,
            }
        }
        Ok(None)
    }
    pub(super) fn get<Q: Query<K> + ?Sized>(&self, query: &Q) -> Option<&V> {
        // No admission callback is supplied: this read-only lookup cannot fail.
        self.search(query, None)
            .expect("unmetered lookup has no fallible operation")
            .map(|id| &self.nodes[id].value)
    }
    pub(super) fn get_governed<Q: Query<K> + ?Sized>(
        &self,
        query: &Q,
        governor: &mut Governor,
    ) -> Result<Option<&V>, StopReason> {
        governor.check()?;
        Ok(self
            .search(query, Some(governor))?
            .map(|id| &self.nodes[id].value))
    }
    pub(super) fn get_mut_governed<Q: Query<K> + ?Sized>(
        &mut self,
        query: &Q,
        governor: &mut Governor,
    ) -> Result<Option<&mut V>, StopReason> {
        governor.check()?;
        let id = self.search(query, Some(governor))?;
        Ok(id.map(|id| &mut self.nodes.as_mut_slice()[id].value))
    }
    pub(super) fn insert(
        &mut self,
        key: K,
        value: V,
        governor: &mut Governor,
    ) -> Result<Option<V>, StopReason> {
        governor.check()?;
        let mut parent = None;
        let mut cursor = self.root;
        let mut direction = Ordering::Equal;
        let mut repair_nodes = 0usize;
        let mut may_rotate = false;
        while let Some(id) = cursor {
            work(governor, Key::units(&key).saturating_add(1))?;
            direction = Key::compare(&key, &self.nodes[id].key);
            if direction == Ordering::Equal {
                work(governor, 1)?;
                return Ok(Some(core::mem::replace(
                    &mut self.nodes.as_mut_slice()[id].value,
                    value,
                )));
            }
            // Admit two child links and their heights before finding the last
            // unbalanced ancestor. Balanced descendants can only grow by one.
            work(governor, 4)?;
            let left = self.height(self.nodes[id].left);
            let right = self.height(self.nodes[id].right);
            repair_nodes += 1;
            if left != right {
                repair_nodes = 1;
                may_rotate = (direction == Ordering::Less) == (left > right);
            }
            parent = Some(id);
            cursor = if direction == Ordering::Less {
                self.nodes[id].left
            } else {
                self.nodes[id].right
            };
        }
        // Admit logical field operations before mutation: leaf/linkage <= 8;
        // each repair ancestor <= 13 (old height, height update, child/height
        // reads and parent); rotation selection <= 4 and each rotation <= 20
        // (relinking plus two height updates). Insertion rotates at most twice
        // globally, and only when the last unbalanced ancestor grows taller.
        governor.charge(
            crate::contract::Resource::Work,
            u64::try_from(repair_nodes)
                .unwrap_or(u64::MAX)
                .saturating_mul(13)
                .saturating_add(8)
                .saturating_add(if may_rotate { 44 } else { 0 }),
        )?;
        if self.nodes.len() == self.nodes.capacity() {
            work(governor, self.nodes.len())?;
        }
        self.nodes.reserve(1)?;
        let id = self.nodes.len();
        self.nodes.push(Node {
            key,
            value,
            parent,
            left: None,
            right: None,
            height: 1,
        })?;
        if let Some(parent) = parent {
            if direction == Ordering::Less {
                self.nodes.as_mut_slice()[parent].left = Some(id);
            } else {
                self.nodes.as_mut_slice()[parent].right = Some(id);
            }
        } else {
            self.root = Some(id);
        }
        self.repair(parent);
        Ok(None)
    }
    /// Restore insertion heights; the complete repair was admitted before linking.
    fn repair(&mut self, mut parent: Option<usize>) {
        while let Some(id) = parent {
            let old_height = self.nodes[id].height;
            if self.height_update(id) == old_height {
                break;
            }
            let left = self.nodes[id].left;
            let right = self.nodes[id].right;
            if self.height(left) > self.height(right) + 1 {
                let left = left.expect("left-heavy node has a left child");
                if self.height(self.nodes[left].right) > self.height(self.nodes[left].left) {
                    self.rotate(left, false);
                }
                self.rotate(id, true);
                break;
            } else if self.height(right) > self.height(left) + 1 {
                let right = right.expect("right-heavy node has a right child");
                if self.height(self.nodes[right].left) > self.height(self.nodes[right].right) {
                    self.rotate(right, true);
                }
                self.rotate(id, false);
                break;
            }
            parent = self.nodes[id].parent;
        }
    }
    fn height(&self, id: Option<usize>) -> u32 {
        id.map_or(0, |id| self.nodes[id].height)
    }
    fn height_update(&mut self, id: usize) -> u32 {
        let height = 1 + self
            .height(self.nodes[id].left)
            .max(self.height(self.nodes[id].right));
        self.nodes.as_mut_slice()[id].height = height;
        height
    }
    fn rotate(&mut self, root: usize, right: bool) -> usize {
        let parent = self.nodes[root].parent;
        let pivot = if right {
            self.nodes[root].left
        } else {
            self.nodes[root].right
        }
        .expect("rotation has its required child");
        let middle = if right {
            self.nodes[pivot].right
        } else {
            self.nodes[pivot].left
        };
        if right {
            self.nodes.as_mut_slice()[root].left = middle;
            self.nodes.as_mut_slice()[pivot].right = Some(root);
        } else {
            self.nodes.as_mut_slice()[root].right = middle;
            self.nodes.as_mut_slice()[pivot].left = Some(root);
        }
        if let Some(middle) = middle {
            self.nodes.as_mut_slice()[middle].parent = Some(root);
        }
        self.nodes.as_mut_slice()[root].parent = Some(pivot);
        self.nodes.as_mut_slice()[pivot].parent = parent;
        if let Some(parent) = parent {
            if self.nodes[parent].left == Some(root) {
                self.nodes.as_mut_slice()[parent].left = Some(pivot);
            } else {
                self.nodes.as_mut_slice()[parent].right = Some(pivot);
            }
        } else {
            self.root = Some(pivot);
        }
        self.height_update(root);
        self.height_update(pivot);
        pivot
    }
    pub(super) fn iter_governed(
        &self,
        governor: &mut Governor,
    ) -> Result<Iter<'_, K, V>, StopReason> {
        self.admit_iteration(governor)?;
        Ok(self.iter())
    }
    pub(super) fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            map: self,
            next: self.root.map(|root| self.minimum(root)),
        }
    }
    fn minimum(&self, mut id: usize) -> usize {
        while let Some(left) = self.nodes[id].left {
            id = left;
        }
        id
    }
    fn successor(&self, mut id: usize) -> Option<usize> {
        if let Some(right) = self.nodes[id].right {
            return Some(self.minimum(right));
        }
        while let Some(parent) = self.nodes[id].parent {
            if self.nodes[parent].left == Some(id) {
                return Some(parent);
            }
            id = parent;
        }
        None
    }
    /// Bounds one iterator step (including initial descent) by tree height.
    fn admit_iteration(&self, governor: &mut Governor) -> Result<(), StopReason> {
        work(
            governor,
            usize::try_from(self.height(self.root))
                .unwrap_or(usize::MAX)
                .saturating_mul(2)
                .saturating_add(1),
        )
    }
}
pub(super) struct Iter<'a, K, V> {
    map: &'a Map<K, V>,
    next: Option<usize>,
}
impl<'a, K: Key, V: ScratchCharge> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next?;
        self.next = self.map.successor(id);
        let node = &self.map.nodes[id];
        Some((&node.key, &node.value))
    }
}
impl<'a, K: Key, V: ScratchCharge> Iter<'a, K, V> {
    pub(super) fn next_governed(
        &mut self,
        governor: &mut Governor,
    ) -> Result<Option<(&'a K, &'a V)>, StopReason> {
        self.map.admit_iteration(governor)?;
        Ok(self.next())
    }
}
impl<K: Key, V: ScratchCharge, Q: Query<K> + ?Sized> Index<&Q> for Map<K, V> {
    type Output = V;
    fn index(&self, key: &Q) -> &V {
        self.get(key)
            .expect("compiled schema index contains its registered key")
    }
}

#[derive(Debug)]
pub(super) struct Arena<T>(ScratchVec<T>);
impl<T: ScratchCharge> Arena<T> {
    pub(super) fn new(governor: &Governor) -> Self {
        Self(ScratchVec::new(governor))
    }
    pub(super) const fn len(&self) -> usize {
        self.0.len()
    }
    pub(super) fn push(&mut self, value: T, governor: &mut Governor) -> Result<(), StopReason> {
        work(governor, 1)?;
        if self.0.len() == self.0.capacity() {
            work(governor, self.0.len())?;
        }
        self.0.push(value)
    }
}
impl<T: ScratchCharge> Index<usize> for Arena<T> {
    type Output = T;
    fn index(&self, id: usize) -> &T {
        &self.0[id]
    }
}
impl<T: ScratchCharge> IndexMut<usize> for Arena<T> {
    fn index_mut(&mut self, id: usize) -> &mut T {
        &mut self.0.as_mut_slice()[id]
    }
}

impl ScratchCharge for super::Vocabularies {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 8);
        8
    };
}
impl ScratchCharge for crate::pattern::Pattern {
    // AST owner (64), captures/engine (16), optional compiled arena (176).
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 256);
        256
    };
}
impl ScratchCharge for super::Location {
    // Dialect/document (16), route (40), pointer/base (80), two maps (112),
    // optional dynamic-anchor owner (40), including portable field padding.
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 288);
        288
    };
}

pub(super) fn route(
    values: &[usize],
    governor: &mut Governor,
) -> Result<ScratchVec<usize>, StopReason> {
    let mut copy = ScratchVec::new(governor);
    copy.reserve(values.len())?;
    for &value in values {
        work(governor, 1)?;
        copy.push(value)?;
    }
    Ok(copy)
}
pub(super) fn pointer(
    parent: &str,
    segment: &str,
    governor: &mut Governor,
) -> Result<ScratchString, StopReason> {
    let mut escapes = 0usize;
    for byte in segment.bytes() {
        work(governor, 1)?;
        escapes += usize::from(matches!(byte, b'~' | b'/'));
    }
    let length = parent
        .len()
        .checked_add(1)
        .and_then(|length| length.checked_add(segment.len()))
        .and_then(|length| length.checked_add(escapes))
        .ok_or(StopReason::BudgetExceeded(
            crate::contract::Resource::ScratchBytes,
        ))?;
    let mut result = ScratchString::new(governor);
    result.reserve(length)?;
    super::reference::append(&mut result, parent, governor)?;
    super::reference::append(&mut result, "/", governor)?;
    let mut start = 0;
    for (position, byte) in segment.bytes().enumerate() {
        work(governor, 1)?;
        let replacement = match byte {
            b'~' => "~0",
            b'/' => "~1",
            _ => continue,
        };
        // ASCII escape characters are always UTF-8 boundaries.
        super::reference::append(&mut result, &segment[start..position], governor)?;
        super::reference::append(&mut result, replacement, governor)?;
        start = position + 1;
    }
    super::reference::append(&mut result, &segment[start..], governor)?;
    Ok(result)
}

pub(super) use crate::contract::push_scratch as push;
pub(super) fn enter_scope(
    schema: &super::Schema,
    id: usize,
    scope: &mut ScratchVec<usize>,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    if let Some(&last) = scope.last() {
        let base = &schema.index.locations[id].base;
        let prior = &schema.index.locations[last].base;
        work(governor, base.len().min(prior.len()).saturating_add(1))?;
        if base == prior {
            return Ok(false);
        }
    }
    push(scope, id, governor)?;
    Ok(true)
}

pub(super) fn child(
    schema: &super::Schema,
    id: usize,
    key: &str,
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    Ok(*schema.index.locations[id]
        .children
        .get_governed(key, governor)?
        .expect("compiled child exists"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Budget, json,
        schema::{CompilationError, Schema},
    };
    use core::fmt::Write as _;

    fn invariant<K: Key, V: ScratchCharge>(map: &Map<K, V>) {
        fn walk<K: Key, V: ScratchCharge>(
            map: &Map<K, V>,
            id: Option<usize>,
            parent: Option<usize>,
            previous: &mut Option<usize>,
        ) -> (u32, usize) {
            let Some(id) = id else {
                return (0, 0);
            };
            let node = &map.nodes[id];
            assert_eq!(node.parent, parent);
            let (left, left_count) = walk(map, node.left, Some(id), previous);
            if let Some(previous) = *previous {
                assert_eq!(
                    Key::compare(&map.nodes[previous].key, &node.key),
                    Ordering::Less
                );
            }
            *previous = Some(id);
            let (right, right_count) = walk(map, node.right, Some(id), previous);
            assert!(left.abs_diff(right) <= 1);
            assert_eq!(node.height, 1 + left.max(right));
            (node.height, left_count + right_count + 1)
        }
        let (_, count) = walk(map, map.root, None, &mut None);
        assert_eq!(count, map.nodes.len());
    }

    #[test]
    fn ordered_schema_indexes_keep_balanced_owners_until_drop_or_late_refusal() {
        bulk_indexes_accept_later_insertions();
        unordered_indexes_keep_last_writes_and_release_on_refusal();
        for reverse in [false, true] {
            let mut source = String::from(r#"{"$ref":"urn:node000","$defs":{"#);
            for offset in 0..64 {
                if offset > 0 {
                    source.push(',');
                }
                let number = if reverse { 63 - offset } else { offset };
                write!(
                    source,
                    r#""node{number:03}":{{"$id":"urn:node{number:03}","$anchor":"node""#
                )
                .unwrap();
                if number == 0 {
                    source.push_str(r#", "pattern":"^x$""#);
                }
                source.push('}');
            }
            source.push_str("}}");
            let mut governor = Governor::new(Budget::default());
            let schema = Schema::compile_governed(&source, &mut governor).unwrap();
            let complete_work = governor.usage().work;
            invariant(&schema.index.by_route);
            invariant(&schema.index.resources);
            invariant(&schema.index.anchors);
            for id in 0..schema.index.locations.len() {
                invariant(&schema.index.locations[id].children);
                invariant(&schema.index.locations[id].patterns);
            }
            assert!(
                schema
                    .validate(&json::parse(r#""x""#).unwrap())
                    .unwrap()
                    .valid
            );
            assert!(
                !schema
                    .validate(&json::parse(r#""xx""#).unwrap())
                    .unwrap()
                    .valid
            );
            let retained = governor.scratch_meter().live();
            drop(schema);
            assert!(retained > 0, "completed indexes retain their actual owners");
            assert_eq!(
                governor.scratch_meter().live(),
                0,
                "schema documents, indexes and patterns release completely"
            );
            let mut refusing = Governor::new(Budget {
                work: complete_work - 1,
                ..Budget::default()
            });
            assert!(matches!(
                Schema::compile_governed(&source, &mut refusing),
                Err(CompilationError::Resource(StopReason::BudgetExceeded(
                    crate::contract::Resource::Work
                )))
            ));
            assert_eq!(
                refusing.scratch_meter().live(),
                0,
                "a late refusal drops every unpublished index and pattern owner"
            );
        }
    }

    fn bulk_indexes_accept_later_insertions() {
        for count in [0usize, 1, 32] {
            let mut governor = Governor::new(Budget::default());
            let mut entries = ScratchVec::new(&governor);
            for key in 0..count {
                entries.push((key * 2, key)).unwrap();
            }
            let mut map = Map::from_sorted(entries, &mut governor).unwrap();
            invariant(&map);
            for key in (0..count).rev() {
                assert_eq!(map.get(&(key * 2)), Some(&key));
                assert_eq!(map.insert(key * 2 + 1, key, &mut governor).unwrap(), None);
                invariant(&map);
            }
            assert_eq!(
                map.insert(0, 99, &mut governor).unwrap(),
                (count > 0).then_some(0)
            );
            invariant(&map);
            assert_eq!(map.get(&0), Some(&99));
            drop(map);
            assert_eq!(governor.scratch_meter().live(), 0);
        }
        for keys in [[1usize, 1], [2, 1]] {
            let mut governor = Governor::new(Budget::default());
            let mut entries = ScratchVec::new(&governor);
            for key in keys {
                entries.push((key, key)).unwrap();
            }
            let Err(StopReason::InvalidInput(message)) = Map::from_sorted(entries, &mut governor)
            else {
                panic!("invalid ordering remains an invariant failure");
            };
            assert_eq!(
                governor.scratch_meter().live(),
                u64::try_from(message.capacity()).unwrap(),
                "only the published invariant diagnostic retains its admission"
            );
        }
    }

    fn unordered_indexes_keep_last_writes_and_release_on_refusal() {
        let mut limit = u64::MAX;
        for refuse in [false, true] {
            let mut governor = Governor::new(Budget {
                work: limit,
                ..Budget::default()
            });
            let mut entries = ScratchVec::new(&governor);
            for (ordinal, key) in [3usize, 1, 3, 2, 1].into_iter().enumerate() {
                entries.push((key, ordinal)).unwrap();
            }
            let result = Map::from_entries(entries, &mut governor);
            if refuse {
                assert!(matches!(
                    result,
                    Err(StopReason::BudgetExceeded(crate::contract::Resource::Work))
                ));
            } else {
                limit = governor.usage().work - 1;
                let mut map = result.unwrap();
                invariant(&map);
                for (key, value) in [(1, 4), (2, 3), (3, 2)] {
                    assert_eq!(map.get(&key), Some(&value));
                }
                for key in [0, 4] {
                    assert_eq!(map.insert(key, key, &mut governor).unwrap(), None);
                    invariant(&map);
                }
                assert_eq!(map.insert(1, 99, &mut governor).unwrap(), Some(4));
                invariant(&map);
            }
            assert_eq!(governor.scratch_meter().live(), 0);
        }
    }
}
