// Copyright (c) The Libra Core Contributors
// SPDX-License-Identifier: Apache-2.0

//! This module implements an in-memory Merkle Accumulator that is similar to what we use in
//! storage. This accumulator will only store a small portion of the tree -- for any subtree that
//! is full, we store only the root. Also we only store the frozen nodes, therefore this structure
//! will always store up to `Log(n)` number of nodes, where `n` is the total number of leaves in
//! the tree.
//!
//! This accumulator is immutable once constructed. If we append new leaves to the tree we will
//! obtain a new accumulator instance and the old one remains unchanged.

#[cfg(test)]
mod accumulator_test;

use super::MerkleTreeInternalNode;
use crypto::{
    hash::{CryptoHash, CryptoHasher, ACCUMULATOR_PLACEHOLDER_HASH},
    HashValue,
};
use failure::prelude::*;
use std::marker::PhantomData;

/// The Accumulator implementation.
pub struct Accumulator<H> {
    /// Represents the roots of all the full subtrees from left to right in this accumulator. For
    /// example, if we have the following accumulator, this vector will have two hashes that
    /// correspond to `X` and `e`.
    /// ```text
    ///                 root
    ///                /    \
    ///              /        \
    ///            /            \
    ///           X              o
    ///         /   \           / \
    ///        /     \         /   \
    ///       o       o       o     placeholder
    ///      / \     / \     / \
    ///     a   b   c   d   e   placeholder
    /// ```
    frozen_subtree_roots: Vec<HashValue>,

    /// The total number of leaves in this accumulator.
    num_leaves: u64,

    /// The root hash of this accumulator.
    root_hash: HashValue,

    phantom: PhantomData<H>,
}

impl<H> Accumulator<H>
where
    H: CryptoHasher,
{
    /// Constructs a new accumulator with roots of existing frozen subtrees. Returns error if the
    /// number of frozen subtree roots does not match the number of leaves.
    pub fn new(frozen_subtree_roots: Vec<HashValue>, num_leaves: u64) -> Result<Self> {
        ensure!(
            frozen_subtree_roots.len() == num_leaves.count_ones() as usize,
            "The number of frozen subtrees does not match the number of leaves. \
             frozen_subtree_roots.len(): {}. num_leaves: {}.",
            frozen_subtree_roots.len(),
            num_leaves,
        );

        let root_hash = Self::compute_root_hash(&frozen_subtree_roots, num_leaves);

        Ok(Self {
            frozen_subtree_roots,
            num_leaves,
            root_hash,
            phantom: PhantomData,
        })
    }

    /// Appends a list of new leaves to an existing accumulator. Since the accumulator is
    /// immutable, the existing one remains unchanged and a new one representing the result is
    /// returned.
    pub fn append(&self, leaves: Vec<HashValue>) -> Self {
        let mut frozen_subtree_roots = self.frozen_subtree_roots.clone();
        let mut num_leaves = self.num_leaves;
        for leaf in leaves {
            Self::append_one(&mut frozen_subtree_roots, num_leaves, leaf);
            num_leaves += 1;
        }

        Self::new(frozen_subtree_roots, num_leaves).expect(
            "Appending leaves to a valid accumulator should create another valid accumulator.",
        )
    }

    /// Appends one leaf. This will update `frozen_subtree_roots` to store new frozen root nodes
    /// and remove old nodes if they are now part of a larger frozen subtree.
    fn append_one(
        frozen_subtree_roots: &mut Vec<HashValue>,
        num_existing_leaves: u64,
        leaf: HashValue,
    ) {
        // For example, this accumulator originally had N = 7 leaves. Appending a leaf is like
        // adding one to this number N: 0b0111 + 1 = 0b1000. Every time we carry a bit to the left
        // we merge the rightmost two subtrees and compute their parent.
        // ```text
        //       A
        //     /   \
        //    /     \
        //   o       o       B
        //  / \     / \     / \
        // o   o   o   o   o   o   o
        // ```

        // First just append the leaf.
        frozen_subtree_roots.push(leaf);

        // Next, merge the last two subtrees into one. If `num_existing_leaves` has N trailing
        // ones, the carry will happen N times.
        let num_trailing_ones = (!num_existing_leaves).trailing_zeros();

        for _i in 0..num_trailing_ones {
            let right_hash = frozen_subtree_roots.pop().expect("Invalid accumulator.");
            let left_hash = frozen_subtree_roots.pop().expect("Invalid accumulator.");
            let parent_hash = MerkleTreeInternalNode::<H>::new(left_hash, right_hash).hash();
            frozen_subtree_roots.push(parent_hash);
        }
    }

    /// Returns the root hash of the accumulator.
    pub fn root_hash(&self) -> HashValue {
        self.root_hash
    }

    /// Computes the root hash of an accumulator given the frozen subtree roots and the number of
    /// leaves in this accumulator.
    fn compute_root_hash(frozen_subtree_roots: &[HashValue], num_leaves: u64) -> HashValue {
        match frozen_subtree_roots.len() {
            0 => return *ACCUMULATOR_PLACEHOLDER_HASH,
            1 => return frozen_subtree_roots[0],
            _ => (),
        }

        // The trailing zeros do not matter since anything below the lowest frozen subtree is
        // already represented by the subtree roots.
        let mut bitmap = num_leaves >> num_leaves.trailing_zeros();
        let mut current_hash = *ACCUMULATOR_PLACEHOLDER_HASH;
        let mut frozen_subtree_iter = frozen_subtree_roots.iter().rev();

        while bitmap > 0 {
            current_hash = if bitmap & 1 != 0 {
                MerkleTreeInternalNode::<H>::new(
                    *frozen_subtree_iter
                        .next()
                        .expect("This frozen subtree should exist."),
                    current_hash,
                )
            } else {
                MerkleTreeInternalNode::<H>::new(current_hash, *ACCUMULATOR_PLACEHOLDER_HASH)
            }
            .hash();
            bitmap >>= 1;
        }

        current_hash
    }

    /// Returns the total number of leaves in this accumulator.
    pub fn num_leaves(&self) -> u64 {
        self.num_leaves
    }
}

// We manually implement Debug because H (CryptoHasher) does not implement Debug.
impl<H> std::fmt::Debug for Accumulator<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Accumulator {{ frozen_subtree_roots: {:?}, num_leaves: {:?} }}",
            self.frozen_subtree_roots, self.num_leaves
        )
    }
}

impl<H> Default for Accumulator<H>
where
    H: CryptoHasher,
{
    fn default() -> Self {
        Accumulator::new(vec![], 0).expect("Constructing empty accumulator should work.")
    }
}
