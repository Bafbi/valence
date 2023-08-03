use std::marker::PhantomData;
use std::mem;
use std::ops::Range;

use valence_core::block_pos::BlockPos;

use super::{Aabb, Bvh, Node, NodeIdx};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct BlockAabb {
    min: BlockPos,
    max: BlockPos,
}

impl BlockAabb {
    fn length_x(self) -> i32 {
        self.max.x - self.min.x
    }

    fn length_y(self) -> i32 {
        self.max.y - self.min.y
    }

    fn length_z(self) -> i32 {
        self.max.z - self.min.z
    }
}

impl From<BlockPos> for BlockAabb {
    fn from(pos: BlockPos) -> Self {
        Self::point(pos)
    }
}

impl Aabb<BlockPos> for BlockAabb {
    fn point(pos: BlockPos) -> Self {
        Self { min: pos, max: pos }
    }

    /// Sum of side lengths.
    fn surface_area(self) -> i32 {
        (self.length_x() + self.length_y() + self.length_z()) * 2
    }

    /// Returns the smallest AABB containing `self` and `other`.
    fn union(self, other: Self) -> Self {
        Self {
            min: BlockPos::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            max: BlockPos::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            ),
        }
    }

    fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }
}

/// Obtains a chunk position for the purpose of placement in the BVH.
pub trait GetBlockPos {
    fn block_pos(&self) -> BlockPos;
}

impl GetBlockPos for BlockPos {
    fn block_pos(&self) -> BlockPos {
        *self
    }
}

pub type BlockBvh<T, const MAX_SURFACE_AREA: i32 = { 1 }> =
    Bvh<T, BlockPos, BlockAabb, MAX_SURFACE_AREA>;

impl<T: GetBlockPos, const MAX_SURFACE_AREA: i32> BlockBvh<T, MAX_SURFACE_AREA> {
    pub fn build(&mut self, items: impl IntoIterator<Item = T>) {
        self.nodes.clear();
        self.values.clear();

        self.values.extend(items);

        if let Some(bounds) = value_bounds(&self.values) {
            self.build_rec(bounds, 0..self.values.len());
        }
    }

    fn build_rec(&mut self, bounds: BlockAabb, value_range: Range<usize>) {
        if bounds.surface_area() <= MAX_SURFACE_AREA {
            self.nodes.push(Node::Leaf {
                bounds,
                values: value_range.start as u32..value_range.end as u32,
            });

            return;
        }

        let values = &mut self.values[value_range.clone()];

        // Determine splitting axis based on the side that's longer. Then split along
        // the spatial midpoint. We could use a more advanced heuristic like SAH,
        // but it's probably not worth it.

        let point =
            if bounds.length_x() >= bounds.length_y() && bounds.length_x() >= bounds.length_z() {
                // Split on Z axis.

                let mid = middle(bounds.min.x, bounds.max.x);
                partition(values, |v| v.block_pos().x >= mid)
            } else if bounds.length_z() >= bounds.length_y() {
                // Split on X axis.

                let mid = middle(bounds.min.z, bounds.max.z);
                partition(values, |v| v.block_pos().z >= mid)
            } else {
                // Split on Y axis.

                let mid = middle(bounds.min.y, bounds.max.y);
                partition(values, |v| v.block_pos().y >= mid)
            };

        let left_range = value_range.start..value_range.start + point;
        let right_range = left_range.end..value_range.end;

        let left_bounds =
            value_bounds(&self.values[left_range.clone()]).expect("left half should be nonempty");

        let right_bounds =
            value_bounds(&self.values[right_range.clone()]).expect("right half should be nonempty");

        self.build_rec(left_bounds, left_range);
        let left_idx = (self.nodes.len() - 1) as NodeIdx;

        self.build_rec(right_bounds, right_range);
        let right_idx = (self.nodes.len() - 1) as NodeIdx;

        self.nodes.push(Node::Internal {
            bounds,
            left: left_idx,
            right: right_idx,
            phantom: PhantomData,
        });
    }

    pub fn query(&self, view: BlockAabb, mut f: impl FnMut(&T)) {
        if let Some(root) = self.nodes.last() {
            self.query_rec(root, view, view, &mut f);
        }
    }

    fn query_rec(
        &self,
        node: &Node<BlockPos, BlockAabb>,
        view: BlockAabb,
        view_aabb: BlockAabb,
        f: &mut impl FnMut(&T),
    ) {
        match node {
            Node::Internal {
                bounds,
                left,
                right,
                ..
            } => {
                if bounds.intersects(view_aabb) {
                    self.query_rec(&self.nodes[*left as usize], view, view_aabb, f);
                    self.query_rec(&self.nodes[*right as usize], view, view_aabb, f);
                }
            }
            Node::Leaf { bounds, values } => {
                if bounds.intersects(view_aabb) {
                    for val in &self.values[values.start as usize..values.end as usize] {
                        if view.intersects(val.block_pos().into()) {
                            f(val)
                        }
                    }
                }
            }
        }
    }

    pub fn shrink_to_fit(&mut self) {
        self.nodes.shrink_to_fit();
        self.values.shrink_to_fit();
    }

    #[cfg(test)]
    fn check_invariants(&self) {
        if let Some(root) = self.nodes.last() {
            self.check_invariants_rec(root);
        }
    }

    #[cfg(test)]
    fn check_invariants_rec(&self, node: &Node<BlockPos, BlockAabb>) {
        match node {
            Node::Internal {
                bounds,
                left,
                right,
                ..
            } => {
                let left = &self.nodes[*left as usize];
                let right = &self.nodes[*right as usize];

                assert_eq!(left.bounds().union(right.bounds()), *bounds);

                self.check_invariants_rec(left);
                self.check_invariants_rec(right);
            }
            Node::Leaf {
                bounds: leaf_bounds,
                values,
            } => {
                let bounds = value_bounds(&self.values[values.start as usize..values.end as usize])
                    .expect("leaf should be nonempty");

                assert_eq!(*leaf_bounds, bounds);
            }
        }
    }
}

fn value_bounds<T: GetBlockPos>(values: &[T]) -> Option<BlockAabb> {
    values
        .iter()
        .map(|v| BlockAabb::point(v.block_pos()))
        .reduce(BlockAabb::union)
}

fn middle(min: i32, max: i32) -> i32 {
    // Cast to i64 to avoid intermediate overflow.
    ((min as i64 + max as i64) / 2) as i32
}

/// Partitions the slice in place and returns the partition point. Why this
/// isn't in Rust's stdlib I don't know.
fn partition<T>(s: &mut [T], mut pred: impl FnMut(&T) -> bool) -> usize {
    let mut it = s.iter_mut();
    let mut true_count = 0;

    while let Some(head) = it.find(|x| {
        if pred(x) {
            true_count += 1;
            false
        } else {
            true
        }
    }) {
        if let Some(tail) = it.rfind(|x| pred(x)) {
            mem::swap(head, tail);
            true_count += 1;
        } else {
            break;
        }
    }
    true_count
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;

    #[test]
    fn partition_middle() {
        let mut arr = [2, 3, 4, 5];
        let mid = middle(arr[0], arr[arr.len() - 1]);

        let point = partition(&mut arr, |&x| mid >= x);

        assert_eq!(point, 2);
        assert_eq!(&arr[..point], &[2, 3]);
        assert_eq!(&arr[point..], &[4, 5]);
    }

    #[test]
    fn query_visits_correct_nodes() {
        let mut bvh = BlockBvh::<BlockPos>::new();

        let mut positions = vec![];

        let size = 500;
        let mut rng = rand::thread_rng();

        // Create a bunch of positions in a large area.
        for _ in 0..100_000 {
            positions.push(BlockPos {
                x: rng.gen_range(-size / 2..size / 2),
                y: rng.gen_range(-size / 2..size / 2),
                z: rng.gen_range(-size / 2..size / 2),
            });
        }

        // Put the view in the center of that area.
        let view = BlockAabb {
            min: BlockPos {
                x: -size / 4,
                y: -size / 4,
                z: -size / 4,
            },
            max: BlockPos {
                x: size / 4,
                y: size / 4,
                z: size / 4,
            },
        };

        let mut viewed_positions = vec![];

        // Create a list of positions the view contains.
        for &pos in &positions {
            if view.intersects(pos.into()) {
                viewed_positions.push(pos);
            }
        }

        bvh.build(positions);

        bvh.check_invariants();

        // Check that we query exactly the positions that we know the view can see.

        bvh.query(view, |pos| {
            let idx = viewed_positions.iter().position(|p| p == pos).expect("😔");
            viewed_positions.remove(idx);
        });

        assert!(viewed_positions.is_empty());
    }
}
