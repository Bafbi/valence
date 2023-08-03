use std::ops::Range;
use std::marker::PhantomData;

pub mod chunk;
pub mod block;

#[derive(Clone, Debug)]
pub(crate) enum Node<U, V>
where
    V: Aabb<U>
{
    Internal {
        bounds: V,
        left: NodeIdx,
        right: NodeIdx,
        phantom: PhantomData<U>,
    },
    Leaf {
        bounds: V,
        /// Range of values in the values array.
        values: Range<NodeIdx>,
    },
}

#[cfg(test)]
impl <V, U> Node<U, V> 
where
    V: Aabb<U> + Copy
{
    fn bounds(&self) -> V {
        match self {
            Node::Internal { bounds, .. } => *bounds,
            Node::Leaf { bounds, .. } => *bounds,
        }
    }
}

type NodeIdx = u32;

pub trait Aabb<T> {

    fn point(pos: T) -> Self;

    fn surface_area(self) -> i32;

    fn union(self, other: Self) -> Self;

    fn intersects(self, other: Self) -> bool;
}

/// A bounding volume hierarchy for chunk positions.
#[derive(Clone, Debug)]
pub struct Bvh<T, U, V, const MAX_SURFACE_AREA: i32 = { 8 * 4 }>
where
    V: Aabb<U>
{
    pub(crate) nodes: Vec<Node<U, V>>,
    pub(crate) values: Vec<T>,
}

impl<T, U, V, const MAX_SURFACE_AREA: i32> Default for Bvh<T, U, V, MAX_SURFACE_AREA>
where
    V: Aabb<U>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U, V, const MAX_SURFACE_AREA: i32> Bvh<T, U, V, MAX_SURFACE_AREA> 
  where
    V: Aabb<U>
{
  pub fn new() -> Self {
      assert!(MAX_SURFACE_AREA > 0);

      Self {
          nodes: vec![],
          values: vec![],
      }
  }
}