use crate::domain::Node;

/// A node in the exact shape expected by the HTTP API.
///
/// The application layer returns plain strings so the HTTP layer only has to
/// serialize them; this module does not depend on `serde` or `axum`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub public_key: String,
    pub alias: String,
    pub capacity: String,
    pub first_seen: String,
}

/// Single entry point for turning a domain `Node` into the API view.
///
/// The actual calculations stay in the domain value objects
/// (`Sats::as_btc` and `FirstSeen::to_rfc3339`), where they are covered by
/// unit tests. Keeping the mapping here means handlers cannot accidentally
/// implement a second, slightly different formatting rule.
#[derive(Debug, Default, Clone, Copy)]
pub struct NodePresenter;

impl NodePresenter {
    pub fn present(&self, node: &Node) -> NodeView {
        NodeView {
            public_key: node.public_key.as_str().to_owned(),
            alias: node.alias.clone(),
            capacity: node.capacity.to_string(),
            first_seen: node.first_seen.to_rfc3339(),
        }
    }

    pub fn present_all(&self, nodes: &[Node]) -> Vec<NodeView> {
        nodes.iter().map(|node| self.present(node)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FirstSeen, PublicKey, Sats};

    fn node(public_key: &str, alias: &str, capacity: u64, first_seen: i64) -> Node {
        Node {
            public_key: PublicKey::parse(public_key).expect("valid public key"),
            alias: alias.to_string(),
            capacity: Sats::new(capacity),
            first_seen: FirstSeen::from_unix(first_seen).expect("valid timestamp"),
            rank_position: 0,
        }
    }

    #[test]
    fn presents_the_api_contract_without_changing_raw_fields() {
        let node = node(
            "03864ef025fde8fb587d989186ce6a4a186895ee44a926bfc370e2c366597a3f8f",
            "ACINQ — Unicode alias",
            36_010_516_297,
            1_522_941_222,
        );

        let view = NodePresenter.present(&node);

        assert_eq!(view.public_key, node.public_key.as_str());
        assert_eq!(view.alias, node.alias);
        assert_eq!(view.capacity, "360.10516297");
        assert_eq!(view.first_seen, "2018-04-05T15:13:42Z");
    }

    #[test]
    fn keeps_capacity_exact_beyond_the_f64_integer_limit() {
        let node = node("03large", "large", (1u64 << 53) + 1, 1_522_941_222);

        assert_eq!(NodePresenter.present(&node).capacity, "90071992.54740993");
    }

    #[test]
    fn preserves_the_snapshot_order() {
        let nodes = vec![
            node("03first", "first", 1, 1_522_941_222),
            node("03second", "second", 2, 1_522_941_223),
        ];

        let views = NodePresenter.present_all(&nodes);

        assert_eq!(
            views
                .iter()
                .map(|view| view.public_key.as_str())
                .collect::<Vec<_>>(),
            vec!["03first", "03second"]
        );
    }
}
