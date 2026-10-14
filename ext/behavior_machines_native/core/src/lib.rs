#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), no_std)]

extern crate alloc;

mod leaf;
#[cfg(feature = "state-machines")]
pub mod machine;
mod node;
mod observe;
mod status;
mod time;
mod tree;

#[cfg(test)]
mod test_support;

pub use leaf::Leaf;
pub use node::{Composite, Limit, Node, Threshold};
pub use observe::{NodeId, Observer};
pub use status::Status;
pub use time::Time;
pub use tree::{BuildError, Tree};
