/// Prefix sums of top-level model sizes and vertical layout extents.
#[derive(Clone, Copy, Default)]
pub(super) struct Extent {
    pub units: usize,
    pub height: f64,
}

impl Extent {
    fn add(self, other: Self) -> Self {
        Self { units: self.units + other.units, height: self.height + other.height }
    }

    fn remove(self, other: Self) -> Self {
        Self { units: self.units - other.units, height: self.height - other.height }
    }
}

#[derive(Default)]
pub(super) struct BlockIndex {
    /// Each entry covers the tree interval ending at its one-based index.
    tree: Vec<Extent>,
}

impl BlockIndex {
    pub fn reset(&mut self, values: impl Iterator<Item = Extent>) {
        self.tree.clear();
        self.tree.push(Extent::default());
        self.tree.extend(values);
        for index in 1..self.tree.len() {
            let parent = index + low_bit(index);
            if parent < self.tree.len() {
                self.tree[parent] = self.tree[parent].add(self.tree[index]);
            }
        }
    }

    pub fn replace(&mut self, index: usize, old: Extent, new: Extent) {
        let mut index = index + 1;
        while index < self.tree.len() {
            self.tree[index] = self.tree[index].remove(old).add(new);
            index += low_bit(index);
        }
    }

    pub fn before(&self, mut index: usize) -> Extent {
        let mut total = Extent::default();
        while index > 0 {
            total = total.add(self.tree[index]);
            index -= low_bit(index);
        }
        total
    }

    /// Number of entries in the longest prefix accepted by a monotone bound.
    pub fn find(&self, accept: impl Fn(Extent) -> bool) -> usize {
        let mut index = 0;
        let mut total = Extent::default();
        let mut stride = self.tree.len().next_power_of_two() / 2;
        while stride > 0 {
            if let Some(next) = self.tree.get(index + stride) {
                let sum = total.add(*next);
                if accept(sum) {
                    total = sum;
                    index += stride;
                }
            }
            stride /= 2;
        }
        index
    }
}

fn low_bit(index: usize) -> usize {
    1 << index.trailing_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_both_axes_after_a_replacement() {
        let list = [Extent { units: 17, height: 20.0 }, Extent { units: 3, height: 10.0 },
            Extent { units: 7, height: 15.0 }];
        let mut index = BlockIndex::default();
        index.reset(list.into_iter());
        assert_eq!(index.find(|extent| extent.units < 17), 0);
        assert_eq!(index.find(|extent| extent.height <= 20.0), 1);
        index.replace(0, list[0], Extent { units: 2, height: 60.0 });
        assert_eq!(index.before(2).units, 5);
        assert_eq!(index.before(2).height, 70.0);
        assert_eq!(index.find(|extent| extent.units < 6), 2);
    }
}
