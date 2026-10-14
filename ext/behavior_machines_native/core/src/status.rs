/// The result of ticking a node.
#[must_use = "a status says whether the node is still running"]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// The node finished and achieved its goal.
    Success,
    /// The node finished without achieving its goal.
    Failure,
    /// The node needs more ticks before it can finish.
    Running,
}
