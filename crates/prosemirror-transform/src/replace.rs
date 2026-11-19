//! Slice fitting and range replacement, ported from prosemirror-transform/src/replace.ts.

use prosemirror_model::{ContentMatch, Fragment, Node, ResolvedPos, Schema, Slice};

use crate::{structure::insert_point, Step, Transform};

pub fn replace_step(doc: &Node, from: usize, to: usize, slice: Slice) -> Option<Step> {
    if from > to || (from == to && slice.size() == 0) {
        return None;
    }
    let start = doc.resolve(from).ok()?;
    let end = doc.resolve(to).ok()?;
    if doc.schema().is_none() || fits_trivially(&start, &end, &slice) {
        return Some(Step::replace(from, to, slice));
    }
    Fitter::new(doc.schema()?, start, end, slice)?.fit()
}

fn fits_trivially(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice) -> bool {
    slice.open_start == 0
        && slice.open_end == 0
        && from.same_parent(to)
        && from.parent().can_replace(from.index(from.depth), to.index(to.depth), &slice.content)
}

struct Frontier {
    node: Node,
    matched: ContentMatch,
}

struct Fittable {
    slice_depth: usize,
    frontier_depth: usize,
    parent: Option<Node>,
    inject: Fragment,
    wrappers: Vec<String>,
}

struct CloseLevel {
    depth: usize,
    fill: Fragment,
    end: ResolvedPos,
}

struct Fitter<'a> {
    schema: &'a Schema,
    from: ResolvedPos,
    to: ResolvedPos,
    frontier: Vec<Frontier>,
    placed: Fragment,
    unplaced: Slice,
}

impl<'a> Fitter<'a> {
    fn new(schema: &'a Schema, from: ResolvedPos, to: ResolvedPos, unplaced: Slice) -> Option<Self> {
        let mut frontier = Vec::new();
        for depth in 0..=from.depth {
            let node = from.node(depth);
            frontier.push(Frontier {
                node: node.clone(),
                matched: node.content_match_at(from.index_after(depth))?,
            });
        }
        let mut placed = Fragment::empty();
        for depth in (1..=from.depth).rev() {
            placed = singleton(from.node(depth).copy(Some(placed)));
        }
        Some(Self { schema, from, to, frontier, placed, unplaced })
    }

    fn depth(&self) -> usize {
        self.frontier.len() - 1
    }

    fn fit(mut self) -> Option<Step> {
        while self.unplaced.size() > 0 {
            if let Some(fittable) = self.find_fittable() {
                self.place_nodes(fittable)?;
            } else if !self.open_more() {
                self.drop_node()?;
            }
        }

        let move_inline = self.must_move_inline();
        let placed_size = self.placed.size().saturating_sub(self.depth() + self.from.depth);
        let end = match move_inline {
            Some(pos) => self.from.doc().resolve(pos).ok()?,
            None => self.to.clone(),
        };
        let end = self.close(&end)?;

        let mut content = self.placed;
        let (mut open_start, mut open_end) = (self.from.depth, end.depth);
        while open_start > 0 && open_end > 0 && content.child_count() == 1 {
            content = content.child(0).content().clone();
            open_start -= 1;
            open_end -= 1;
        }
        let slice = Slice::new(content, open_start, open_end);
        if let Some(pos) = move_inline {
            return Some(Step::replace_around(
                self.from.pos, pos, self.to.pos, self.to.end(self.to.depth),
                slice, placed_size, false,
            ));
        }
        if slice.size() > 0 || self.from.pos != self.to.pos {
            Some(Step::replace(self.from.pos, end.pos, slice))
        } else {
            None
        }
    }

    fn find_fittable(&self) -> Option<Fittable> {
        let mut start_depth = self.unplaced.open_start;
        let mut content = &self.unplaced.content;
        let mut open_end = self.unplaced.open_end;
        for depth in 0..start_depth {
            let node = content.children.first()?;
            if content.child_count() > 1 {
                open_end = 0;
            }
            if node.is_isolating() && open_end <= depth {
                start_depth = depth;
                break;
            }
            content = node.content();
        }
        // Direct placement and filler have priority over adding wrappers.
        for (wrap, max_depth) in [(false, start_depth), (true, self.unplaced.open_start)] {
            for slice_depth in (0..=max_depth).rev() {
                let parent = if slice_depth == 0 {
                    None
                } else {
                    content_at(&self.unplaced.content, slice_depth - 1)?.children.first()
                };
                let fragment = parent.map_or(&self.unplaced.content, Node::content);
                for (frontier_depth, frontier) in self.frontier.iter().enumerate().rev() {
                    let mut inject = Fragment::empty();
                    let mut wrappers = Vec::new();
                    let fits = match (wrap, fragment.children.first()) {
                        (false, Some(first)) => {
                            if frontier.matched.match_type(first.type_name()).is_some() {
                                true
                            } else if let Some(fill) = frontier.matched.fill_before(
                                self.schema, &singleton(first.clone()), false, 0,
                            ) {
                                inject = fill;
                                true
                            } else {
                                false
                            }
                        }
                        (false, None) => parent.is_some_and(|node| frontier.node.compatible_content(node)),
                        (true, Some(first)) => {
                            if let Some(found) = frontier.matched.find_wrapping(self.schema, first.type_name()) {
                                wrappers = found;
                                true
                            } else {
                                false
                            }
                        }
                        (true, None) => false,
                    };
                    if fits {
                        return Some(Fittable {
                            slice_depth, frontier_depth, parent: parent.cloned(), inject, wrappers,
                        });
                    }
                    if parent.is_some_and(|node| frontier.matched.match_type(node.type_name()).is_some()) {
                        break;
                    }
                }
            }
        }
        None
    }

    fn open_more(&mut self) -> bool {
        let slice = &mut self.unplaced;
        if max_open(&slice.content, true) <= slice.open_start {
            return false;
        }
        if slice.size() > 1 && max_open(&slice.content, false) > slice.open_end {
            slice.open_end += 1;
        }
        slice.open_start += 1;
        true
    }

    fn drop_node(&mut self) -> Option<()> {
        let Slice { content, open_start, open_end } = &self.unplaced;
        let inner = content_at(content, *open_start)?;
        self.unplaced = if inner.child_count() <= 1 && *open_start > 0 {
            let open_at_end = content.size() - open_start <= open_start + inner.size();
            Slice::new(
                drop_from_fragment(content, open_start - 1, 1)?,
                open_start - 1,
                if open_at_end { open_start - 1 } else { *open_end },
            )
        } else {
            Slice::new(drop_from_fragment(content, *open_start, 1)?, *open_start, *open_end)
        };
        Some(())
    }

    fn place_nodes(&mut self, fit: Fittable) -> Option<()> {
        while self.depth() > fit.frontier_depth {
            self.close_frontier_node()?;
        }
        for wrapper in fit.wrappers {
            let node = self.schema.node_type(&wrapper)?.try_create(None, None, None).ok()?;
            self.open_frontier_node(node)?;
        }

        let slice = self.unplaced.clone();
        let fragment = fit.parent.as_ref().map_or(&slice.content, Node::content);
        let open_start = slice.open_start - fit.slice_depth;
        let mut open_end = (fragment.size() + fit.slice_depth) as isize
            - (slice.content.size() - slice.open_end) as isize;
        let frontier = &mut self.frontier[fit.frontier_depth];
        let mut matched = frontier.matched.match_fragment(&fit.inject, 0, fit.inject.child_count())?;
        let mut added = fit.inject.children;
        let mut taken = 0;
        for next in &fragment.children {
            let Some(matches) = matched.match_type(next.type_name()) else { break };
            taken += 1;
            if taken > 1 || open_start == 0 || next.content().size() > 0 {
                matched = matches;
                let marks = next.marks().iter()
                    .filter(|mark| frontier.node.allows_mark_type(&mark.type_name))
                    .cloned().collect();
                added.push(close_node_start(
                    self.schema,
                    &next.with_marks(marks),
                    if taken == 1 { open_start } else { 0 },
                    if taken == fragment.child_count() { open_end } else { -1 },
                )?);
            }
        }
        let to_end = taken == fragment.child_count();
        if !to_end {
            open_end = -1;
        }
        let added = Fragment::from_nodes(added);
        self.placed = add_to_fragment(&self.placed, fit.frontier_depth, &added)?;
        frontier.matched = matched;
        if to_end && open_end < 0 && self.depth() > 0
            && fit.parent.as_ref().is_some_and(|parent| parent.type_name() == self.frontier[self.depth()].node.type_name())
        {
            self.close_frontier_node()?;
        }
        for node in std::iter::successors(added.last_child(), |node| node.last_child())
            .take(open_end.max(0) as usize)
        {
            self.frontier.push(Frontier {
                node: node.clone(),
                matched: node.content_match_at(node.child_count())?,
            });
        }

        self.unplaced = if !to_end {
            Slice::new(
                drop_from_fragment(&slice.content, fit.slice_depth, taken)?,
                fit.slice_depth,
                slice.open_end,
            )
        } else if fit.slice_depth == 0 {
            Slice::empty()
        } else {
            Slice::new(
                drop_from_fragment(&slice.content, fit.slice_depth - 1, 1)?,
                fit.slice_depth - 1,
                if open_end < 0 { slice.open_end } else { fit.slice_depth - 1 },
            )
        };
        Some(())
    }

    fn must_move_inline(&self) -> Option<usize> {
        let top = self.frontier.last()?;
        if !self.to.parent().is_textblock()
            || !top.node.is_textblock()
            || self.content_after_fits(&self.to, self.to.depth, top, false).is_none()
            || (self.to.depth == self.depth()
                && self.find_close_level(&self.to).is_some_and(|level| level.depth == self.depth()))
        {
            return None;
        }
        let mut depth = self.to.depth;
        let mut after = self.to.after(depth);
        while depth > 1 {
            depth -= 1;
            if after != self.to.end(depth) {
                break;
            }
            after += 1;
        }
        Some(after)
    }

    fn find_close_level(&self, to: &ResolvedPos) -> Option<CloseLevel> {
        'scan: for depth in (0..=self.depth().min(to.depth)).rev() {
            let drop_inner = depth < to.depth && to.end(depth + 1) == to.pos + to.depth - depth - 1;
            let Some(fill) = self.content_after_fits(to, depth, &self.frontier[depth], drop_inner) else {
                continue;
            };
            for outer in (0..depth).rev() {
                if self.content_after_fits(to, outer, &self.frontier[outer], true)
                    .is_none_or(|fragment| fragment.child_count() != 0)
                {
                    continue 'scan;
                }
            }
            let end = if drop_inner { to.doc().resolve(to.after(depth + 1)).ok()? } else { to.clone() };
            return Some(CloseLevel { depth, fill, end });
        }
        None
    }

    fn content_after_fits(
        &self, to: &ResolvedPos, depth: usize, frontier: &Frontier, open: bool,
    ) -> Option<Fragment> {
        let node = to.node(depth);
        let index = if open { to.index_after(depth) } else { to.index(depth) };
        if index == node.child_count() && !frontier.node.compatible_content(node) {
            return None;
        }
        if node.content().children[index..].iter().any(|child| {
            child.marks().iter().any(|mark| !frontier.node.allows_mark_type(&mark.type_name))
        }) {
            return None;
        }
        frontier.matched.fill_before(self.schema, node.content(), true, index)
    }

    fn close(&mut self, to: &ResolvedPos) -> Option<ResolvedPos> {
        let close = self.find_close_level(to)?;
        while self.depth() > close.depth {
            self.close_frontier_node()?;
        }
        if close.fill.child_count() > 0 {
            self.placed = add_to_fragment(&self.placed, close.depth, &close.fill)?;
        }
        for depth in close.depth + 1..=close.end.depth {
            let node = close.end.node(depth);
            let fill = node.content_match_at(0)?.fill_before(self.schema, node.content(), true, close.end.index(depth))?;
            self.open_frontier_node(node.copy(Some(fill)))?;
        }
        Some(close.end)
    }

    fn open_frontier_node(&mut self, node: Node) -> Option<()> {
        let depth = self.depth();
        let top = self.frontier.last_mut()?;
        top.matched = top.matched.match_type(node.type_name())?;
        self.placed = add_to_fragment(&self.placed, depth, &singleton(node.clone()))?;
        self.frontier.push(Frontier { matched: node.content_match_at(0)?, node });
        Some(())
    }

    fn close_frontier_node(&mut self) -> Option<()> {
        let open = self.frontier.pop()?;
        let fill = open.matched.fill_before(self.schema, &Fragment::empty(), true, 0)?;
        if fill.child_count() > 0 {
            self.placed = add_to_fragment(&self.placed, self.frontier.len(), &fill)?;
        }
        Some(())
    }
}

pub(crate) fn singleton(node: Node) -> Fragment {
    Fragment::from_nodes(vec![node])
}

fn content_at(mut fragment: &Fragment, depth: usize) -> Option<&Fragment> {
    for _ in 0..depth {
        fragment = fragment.children.first()?.content();
    }
    Some(fragment)
}

fn drop_from_fragment(fragment: &Fragment, depth: usize, count: usize) -> Option<Fragment> {
    if depth == 0 {
        return (count <= fragment.child_count())
            .then(|| fragment.cut_by_index(count, fragment.child_count()));
    }
    let first = fragment.children.first()?;
    let child = first.copy(Some(drop_from_fragment(first.content(), depth - 1, count)?));
    Some(fragment.replace_child(0, child))
}

fn add_to_fragment(fragment: &Fragment, depth: usize, content: &Fragment) -> Option<Fragment> {
    if depth == 0 {
        return Some(fragment.append(content));
    }
    let last = fragment.children.last()?;
    let child = last.copy(Some(add_to_fragment(last.content(), depth - 1, content)?));
    Some(fragment.replace_child(fragment.child_count() - 1, child))
}

fn close_node_start(schema: &Schema, node: &Node, open_start: usize, open_end: isize) -> Option<Node> {
    if open_start == 0 {
        return Some(node.clone());
    }
    let mut fragment = node.content().clone();
    if open_start > 1 {
        let child = close_node_start(
            schema, fragment.children.first()?, open_start - 1,
            if fragment.child_count() == 1 { open_end - 1 } else { 0 },
        )?;
        fragment = fragment.replace_child(0, child);
    }
    let matched = node.content_match_at(0)?;
    fragment = matched.fill_before(schema, &fragment, false, 0)?.append(&fragment);
    if open_end <= 0 {
        let tail = matched.match_fragment(&fragment, 0, fragment.child_count())?
            .fill_before(schema, &Fragment::empty(), true, 0)?;
        fragment = fragment.append(&tail);
    }
    Some(node.copy(Some(fragment)))
}

fn close_fragment(
    fragment: &Fragment, depth: usize, old_open: usize, new_open: usize, parent: &Node,
) -> Option<Fragment> {
    let mut fragment = fragment.clone();
    if depth < old_open {
        let first = fragment.children.first()?;
        let inner = close_fragment(first.content(), depth + 1, old_open, new_open, first)?;
        fragment = fragment.replace_child(0, first.copy(Some(inner)));
    }
    if depth > new_open {
        let schema = parent.schema()?;
        let matched = parent.content_match_at(0)?;
        fragment = matched.fill_before(schema, &fragment, false, 0)?.append(&fragment);
        let tail = matched.match_fragment(&fragment, 0, fragment.child_count())?
            .fill_before(schema, &Fragment::empty(), true, 0)?;
        fragment = fragment.append(&tail);
    }
    Some(fragment)
}

pub(crate) fn replace_range(transform: &mut Transform, from: usize, to: usize, slice: Slice) {
    if slice.size() == 0 {
        delete_range(transform, from, to);
        return;
    }
    let (Ok(start), Ok(end)) = (transform.doc.resolve(from), transform.doc.resolve(to)) else { return };
    if transform.doc.schema().is_none() || fits_trivially(&start, &end, &slice) {
        transform.maybe_step(Step::replace(from, to, slice));
        return;
    }

    let mut target_depths: Vec<isize> = covered_depths(&start, &end).into_iter()
        .filter(|depth| *depth > 0).map(|depth| depth as isize).collect();
    let mut preferred_target = -(start.depth as isize + 1);
    target_depths.insert(0, preferred_target);
    for depth in (1..=start.depth).rev() {
        let node = start.node(depth);
        let defines_context = node.node_type().is_some_and(|ty| ty.spec.defining || ty.spec.defining_as_context);
        if defines_context || node.is_isolating() {
            break;
        }
        if target_depths.contains(&(depth as isize)) {
            preferred_target = depth as isize;
        } else if start.before(depth) + (start.depth - depth + 1) == from {
            target_depths.insert(1, -(depth as isize));
        }
    }
    let preferred_index = target_depths.iter().position(|depth| *depth == preferred_target).unwrap_or(0);
    let mut left_nodes = Vec::new();
    let mut content = &slice.content;
    for _ in 0..=slice.open_start {
        let Some(node) = content.children.first() else { break };
        left_nodes.push(node);
        content = node.content();
    }
    let mut preferred_depth = slice.open_start;
    for depth in (0..preferred_depth.min(left_nodes.len())).rev() {
        let node = left_nodes[depth];
        let defines = node.node_type().is_some_and(|ty| ty.spec.defining || ty.spec.defining_for_content);
        if defines && !node.same_markup(start.node(preferred_target.unsigned_abs() - 1)) {
            preferred_depth = depth;
        } else if defines || !node.is_textblock() {
            break;
        }
    }

    for offset in (0..=slice.open_start).rev() {
        let open_depth = (offset + preferred_depth + 1) % (slice.open_start + 1);
        let Some(insert) = left_nodes.get(open_depth) else { continue };
        for index in 0..target_depths.len() {
            let target = target_depths[(index + preferred_index) % target_depths.len()];
            let depth = target.unsigned_abs();
            let parent = start.node(depth - 1);
            let index = start.index(depth - 1);
            if parent.can_replace_with(index, index, insert.type_name(), insert.marks()) {
                if let Some(content) = close_fragment(&slice.content, 0, slice.open_start, open_depth, parent) {
                    transform.replace(
                        start.before(depth),
                        if target > 0 { end.after(depth) } else { to },
                        Slice::new(content, open_depth, slice.open_end),
                    );
                    return;
                }
            }
        }
    }

    let (mut from, mut to) = (from, to);
    let original_steps = transform.steps.len();
    for target in target_depths.into_iter().rev() {
        transform.replace(from, to, slice.clone());
        if transform.steps.len() > original_steps {
            break;
        }
        if target > 0 {
            from = start.before(target as usize);
            to = end.after(target as usize);
        }
    }
}

pub(crate) fn replace_range_with(transform: &mut Transform, from: usize, to: usize, node: Node) {
    let (mut from, mut to) = (from, to);
    if !node.is_inline() && from == to
        && transform.doc.resolve(from).is_ok_and(|resolved| resolved.parent().content().size() > 0)
    {
        if let Some(point) = insert_point(&transform.doc, from, node.type_name()) {
            from = point;
            to = point;
        }
    }
    replace_range(transform, from, to, Slice::new(singleton(node), 0, 0));
}

pub(crate) fn delete_range(transform: &mut Transform, mut from: usize, mut to: usize) {
    let (Ok(mut start), Ok(mut end)) = (transform.doc.resolve(from), transform.doc.resolve(to)) else { return };
    if start.parent().is_textblock() && end.parent().is_textblock()
        && start.start(start.depth) != end.start(end.depth)
        && start.parent_offset == 0 && end.parent_offset == 0
    {
        let shared = start.shared_depth(to);
        let isolating = |pos: &ResolvedPos| (shared + 1..=pos.depth).any(|depth| pos.node(depth).is_isolating());
        if !isolating(&start) && !isolating(&end) {
            for depth in (1..=start.depth).rev() {
                if from != start.start(depth) { break }
                from = start.before(depth);
            }
            for depth in (1..=end.depth).rev() {
                if to != end.start(depth) { break }
                to = end.before(depth);
            }
            let (Ok(next_start), Ok(next_end)) = (transform.doc.resolve(from), transform.doc.resolve(to)) else { return };
            (start, end) = (next_start, next_end);
        }
    }
    let covered = covered_depths(&start, &end);
    for (index, depth) in covered.iter().copied().enumerate() {
        let last = index + 1 == covered.len();
        if (last && depth == 0) || start.node(depth).content_match_at(0).is_some_and(|m| m.valid_end()) {
            transform.delete(start.start(depth), end.end(depth));
            return;
        }
        if depth > 0
            && (last || start.node(depth - 1).can_replace(
                start.index(depth - 1), end.index_after(depth - 1), &Fragment::empty(),
            ))
        {
            transform.delete(start.before(depth), end.after(depth));
            return;
        }
    }
    for depth in 1..=start.depth.min(end.depth) {
        if from - start.start(depth) == start.depth - depth && to > start.end(depth)
            && end.end(depth) - to != end.depth - depth
            && start.start(depth - 1) == end.start(depth - 1)
            && start.node(depth - 1).can_replace(start.index(depth - 1), end.index(depth - 1), &Fragment::empty())
        {
            transform.delete(start.before(depth), to);
            return;
        }
    }
    transform.delete(from, to);
}

fn covered_depths(from: &ResolvedPos, to: &ResolvedPos) -> Vec<usize> {
    let mut covered = Vec::new();
    for depth in (0..=from.depth.min(to.depth)).rev() {
        let start = from.start(depth);
        if start < from.pos - (from.depth - depth)
            || to.end(depth) > to.pos + to.depth - depth
            || from.node(depth).is_isolating() || to.node(depth).is_isolating()
        {
            break;
        }
        if start == to.start(depth) || (depth > 0 && depth == from.depth && depth == to.depth
            && from.parent().inline_content() && to.parent().inline_content()
            && to.start(depth - 1) + 1 == start)
        {
            covered.push(depth);
        }
    }
    covered
}

fn max_open(mut fragment: &Fragment, from_left: bool) -> usize {
    let mut count = 0;
    loop {
        let child = if from_left { fragment.first_child() } else { fragment.last_child() };
        let Some(child) = child.filter(|node| !node.is_atom()) else { return count };
        count += 1;
        fragment = child.content();
    }
}
