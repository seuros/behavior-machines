use core::time::Duration;

use crate::{Limit, Node, Threshold};

/// Every kind of child slot, with leaves in declaration order.
fn shape<L>([check, then, other, left, right, guard, body, retry]: [L; 8]) -> Node<L> {
    Node::sequence([
        Node::if_then_else(Node::leaf(check), Node::leaf(then), Node::leaf(other)),
        Node::parallel(
            [Node::leaf(left), Node::leaf(right).inverter()],
            Threshold::All,
            Threshold::Count(1),
        ),
        Node::while_do(
            Node::leaf(guard),
            Node::leaf(body).timeout(Duration::from_millis(5)),
        ),
        Node::reactive_fallback([
            Node::AlwaysFailure,
            Node::leaf(retry).repeat(Limit::Times(2)),
        ]),
    ])
}

#[test]
fn map_visits_leaves_in_node_id_order_and_keeps_the_shape() {
    let names = ["c", "t", "e", "p1", "p2", "w", "b", "r"];
    let mut seen = Vec::new();
    let numbered = shape(names).map(|name| {
        seen.push(name);
        seen.len() - 1
    });

    assert_eq!(seen, names);
    assert_eq!(numbered, shape([0, 1, 2, 3, 4, 5, 6, 7]));
}
