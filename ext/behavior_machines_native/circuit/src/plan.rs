//! Where each input cell lives and what it means.
//!
//! The circuit reads one flat `[f32]`. The plan is the only thing that knows
//! how to address it, so the layout can change without touching the lowering
//! or the host.
//!
//! ```text
//! [ running: one per node ][ cursor: one per memory composite ][ leaf status: three per leaf ]
//! ```
//!
//! `running` and `cursor` are the tree's state carried between ticks, the same
//! fields the engine keeps in each slot. The leaf statuses are this tick's
//! input: the host evaluates its leaves and writes them here.

use alloc::vec::Vec;

use behavior_machines::Status;

use crate::builder::{Bit, Builder};
use crate::status::StatusBits;
use crate::wire::Wire;

/// The input-cell layout of a lowered tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    nodes: usize,
    /// Node index of each leaf slot, in leaf order.
    leaves: Vec<usize>,
    /// Node index of each cursor cell, in cell order.
    cursors: Vec<usize>,
    /// Every node index in post-order.
    report_order: Vec<usize>,
}

impl Plan {
    pub(crate) const fn new(
        nodes: usize,
        leaves: Vec<usize>,
        cursors: Vec<usize>,
        report_order: Vec<usize>,
    ) -> Self {
        Self {
            nodes,
            leaves,
            cursors,
            report_order,
        }
    }

    /// Every node index in the order the engine's
    /// [`Observer`](behavior_machines::Observer) reports them: depth-first
    /// post-order, children before parents.
    ///
    /// Reversing the pre-order indices is *not* this order. Pre-order is
    /// `[parent, subtree(c0), subtree(c1)]`, so reversing it yields `c1`'s
    /// subtree before `c0`'s, while the engine ticks `c0` first.
    #[must_use]
    pub fn report_order(&self) -> &[usize] {
        &self.report_order
    }

    /// How many nodes the tree has. Node indices are the engine's
    /// [`NodeId`](behavior_machines::NodeId) values: both number nodes by a
    /// depth-first pre-order walk of the definition.
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.nodes
    }

    /// How many leaves the tree has.
    #[must_use]
    pub const fn leaf_count(&self) -> usize {
        self.leaves.len()
    }

    /// How many memory composites the tree has, each owning one cursor cell.
    #[must_use]
    pub const fn cursor_count(&self) -> usize {
        self.cursors.len()
    }

    /// The node index of leaf slot `slot`.
    #[must_use]
    pub fn leaf_node(&self, slot: usize) -> Option<usize> {
        self.leaves.get(slot).copied()
    }

    /// The leaf slot of node `index`, if it is a leaf.
    #[must_use]
    pub fn leaf_slot(&self, index: usize) -> Option<usize> {
        self.leaves.iter().position(|node| *node == index)
    }

    /// The node index owning cursor cell `cell`.
    #[must_use]
    pub fn cursor_node(&self, cell: usize) -> Option<usize> {
        self.cursors.get(cell).copied()
    }

    /// How many input cells a tick needs.
    #[must_use]
    pub const fn cell_count(&self) -> u32 {
        let cells = self.nodes + self.cursors.len() + self.leaves.len() * 3;
        // A tree large enough to overflow this would not fit in memory.
        #[expect(clippy::cast_possible_truncation, reason = "bounded by tree size")]
        let cells = cells as u32;
        cells
    }

    /// The cell holding node `index`'s `running` flag.
    #[must_use]
    pub const fn running_cell(&self, index: usize) -> u32 {
        #[expect(clippy::cast_possible_truncation, reason = "bounded by tree size")]
        let cell = index as u32;
        cell
    }

    /// The cell holding cursor `cell`.
    #[must_use]
    pub const fn cursor_cell(&self, cell: usize) -> u32 {
        #[expect(clippy::cast_possible_truncation, reason = "bounded by tree size")]
        let offset = (self.nodes + cell) as u32;
        offset
    }

    /// The first of the three cells holding leaf `slot`'s status, in
    /// success, failure, running order.
    #[must_use]
    pub const fn leaf_cell(&self, slot: usize) -> u32 {
        #[expect(clippy::cast_possible_truncation, reason = "bounded by tree size")]
        let offset = (self.nodes + self.cursors.len() + slot * 3) as u32;
        offset
    }

    /// The cell holding leaf `slot`'s `status` bit.
    #[must_use]
    pub const fn leaf_status_cell(&self, slot: usize, status: Status) -> u32 {
        self.leaf_cell(slot) + status_offset(status)
    }

    pub(crate) fn running_wire(&self, index: usize, builder: &Builder) -> Wire {
        builder.input(self.running_cell(index))
    }

    pub(crate) fn cursor_wire(&self, cell: usize, builder: &Builder) -> Wire {
        builder.input(self.cursor_cell(cell))
    }

    /// Leaf `slot`'s status, read from its three cells.
    ///
    /// The host promises the three hold a one-hot triple; a leaf always
    /// returns exactly one of the three statuses, so that is what writing the
    /// result down produces.
    pub(crate) fn leaf_status(&self, slot: usize, builder: &Builder) -> StatusBits {
        let bit = |status| Bit::assume(builder.input(self.leaf_status_cell(slot, status)));
        StatusBits {
            success: bit(Status::Success),
            failure: bit(Status::Failure),
            running: bit(Status::Running),
        }
    }
}

/// The offset of a status within a leaf's three cells.
const fn status_offset(status: Status) -> u32 {
    match status {
        Status::Success => 0,
        Status::Failure => 1,
        Status::Running => 2,
    }
}

#[cfg(test)]
mod tests;
