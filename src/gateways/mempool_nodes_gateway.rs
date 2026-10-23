use std::collections::HashSet;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

use crate::domain::{DomainError, FirstSeen, Node, PublicKey, Sats};
use crate::gateways::nodes_gateway::{GatewayError, NodesGateway};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawNode {
    pub public_key: Option<String>,
    pub alias: Option<String>,
    pub capacity: Option<i64>,
    pub first_seen: Option<i64>,
}

impl RawNode {
    fn into_domain(self, rank_position: u32) -> Result<Node, DomainError> {
        let public_key = PublicKey::parse(self.public_key.unwrap_or_default())?;

        let capacity = self.capacity.ok_or(DomainError::MissingCapacity)?;
        let capacity =
            u64::try_from(capacity).map_err(|_| DomainError::NegativeCapacity(capacity))?;

        let first_seen =
            FirstSeen::from_unix(self.first_seen.ok_or(DomainError::MissingFirstSeen)?)?;

        Ok(Node {
            public_key,
            alias: self.alias.unwrap_or_default(),
            capacity: Sats::new(capacity),
            first_seen,
            rank_position,
        })
    }
}

#[derive(Debug, Default)]
pub struct ParseOutcome {
    pub nodes: Vec<Node>,
    pub errors: Vec<DomainError>,
}

pub fn parse_nodes(raw: Vec<RawNode>) -> ParseOutcome {
    let mut nodes = Vec::with_capacity(raw.len());
    let mut errors = Vec::new();
    let mut seen = HashSet::new();

    for (rank, item) in raw.into_iter().enumerate() {
        let rank_position = rank as u32;
        match item.into_domain(rank_position) {
            Ok(node) if seen.insert(node.public_key.as_str().to_owned()) => {
                nodes.push(node);
            }
            Ok(node) => errors.push(DomainError::DuplicatePublicKey(
                node.public_key.as_str().to_owned(),
            )),
            Err(error) => errors.push(error),
        }
    }

    ParseOutcome { nodes, errors }
}

pub struct MempoolNodesGateway {
    client: Client,
    url: String,
}

impl MempoolNodesGateway {
    pub fn new(url: impl Into<String>) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("rust-lightning-service/0.1")
            .build()?;

        Ok(Self {
            client,
            url: url.into(),
        })
    }
}

impl NodesGateway for MempoolNodesGateway {
    async fn fetch(&self) -> Result<Vec<Node>, GatewayError> {
        let response = self.client.get(&self.url).send().await.map_err(|error| {
            tracing::error!(%error, "mempool request failed");
            GatewayError::Request
        })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            tracing::error!(status, "mempool returned a non-success status");
            return Err(GatewayError::HttpStatus(status));
        }

        let raw = response.json::<Vec<RawNode>>().await.map_err(|error| {
            tracing::error!(%error, "failed to decode the mempool response");
            GatewayError::InvalidPayload
        })?;

        let outcome = parse_nodes(raw);
        for error in &outcome.errors {
            tracing::warn!(%error, "skipping invalid mempool node");
        }

        Ok(outcome.nodes)
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn raw_node(public_key: &str, capacity: i64, first_seen: i64) -> RawNode {
        RawNode {
            public_key: Some(public_key.to_string()),
            alias: Some("node".to_string()),
            capacity: Some(capacity),
            first_seen: Some(first_seen),
        }
    }

    #[test]
    fn parses_valid_nodes_and_preserves_rank() {
        let outcome = parse_nodes(vec![
            raw_node("node-a", 10_000, 1_522_941_222),
            raw_node("node-b", 20_000, 1_522_941_223),
        ]);

        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.nodes.len(), 2);
        assert_eq!(outcome.nodes[0].rank_position, 0);
        assert_eq!(outcome.nodes[1].rank_position, 1);
        assert_eq!(outcome.nodes[0].capacity.as_btc(), "0.00010000");
    }

    #[test]
    fn skips_invalid_and_duplicate_nodes() {
        let outcome = parse_nodes(vec![
            raw_node("node-a", 10_000, 1_522_941_222),
            raw_node("node-a", 20_000, 1_522_941_223),
            raw_node("", 30_000, 1_522_941_224),
            RawNode {
                public_key: Some("node-b".to_string()),
                alias: None,
                capacity: Some(-1),
                first_seen: Some(1_522_941_225),
            },
        ]);

        assert_eq!(outcome.nodes.len(), 1);
        assert_eq!(outcome.nodes[0].public_key.as_str(), "node-a");
        assert_eq!(outcome.errors.len(), 3);
    }

    #[test]
    fn ignores_unknown_json_fields() {
        let raw: Vec<RawNode> = serde_json::from_str(
            r#"[{
                "publicKey": "node-a",
                "alias": "ACINQ",
                "capacity": 36010516297,
                "firstSeen": 1522941222,
                "channels": 2908,
                "country": {"en": "United States"}
            }]"#,
        )
        .expect("valid payload");

        let outcome = parse_nodes(raw);

        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.nodes.len(), 1);
        assert_eq!(outcome.nodes[0].capacity.as_btc(), "360.10516297");
    }
}

#[cfg(test)]
mod integration_tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    const NODES_PATH: &str = "/api/v1/lightning/nodes/rankings/connectivity";

    fn json_response(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .insert_header("content-type", "application/json")
            .set_body_string(body)
    }

    fn gateway_for(server: &MockServer) -> MempoolNodesGateway {
        MempoolNodesGateway::new(format!("{}{NODES_PATH}", server.uri()))
            .expect("build the mempool gateway")
    }

    #[tokio::test]
    async fn fetches_and_validates_nodes_over_http() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(NODES_PATH))
            .respond_with(json_response(
                r#"[
                    {
                        "publicKey": "node-a",
                        "alias": "ACINQ",
                        "capacity": 36010516297,
                        "firstSeen": 1522941222,
                        "channels": 2908,
                        "country": {"en": "United States"}
                    },
                    {
                        "publicKey": "node-b",
                        "alias": "WalletOfSatoshi.com",
                        "capacity": 100000,
                        "firstSeen": 1522941223
                    }
                ]"#,
            ))
            .expect(1)
            .mount(&server)
            .await;

        let nodes = gateway_for(&server)
            .fetch()
            .await
            .expect("fetch nodes from the mock server");

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].public_key.as_str(), "node-a");
        assert_eq!(nodes[0].alias, "ACINQ");
        assert_eq!(nodes[0].capacity.as_btc(), "360.10516297");
        assert_eq!(nodes[0].rank_position, 0);
        assert_eq!(nodes[1].rank_position, 1);
    }

    #[tokio::test]
    async fn keeps_valid_nodes_and_skips_invalid_ones() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(NODES_PATH))
            .respond_with(json_response(
                r#"[
                    {
                        "publicKey": "node-a",
                        "alias": "valid",
                        "capacity": 1000,
                        "firstSeen": 1522941222
                    },
                    {
                        "publicKey": "",
                        "alias": "missing public key",
                        "capacity": 2000,
                        "firstSeen": 1522941223
                    },
                    {
                        "publicKey": "node-b",
                        "alias": "negative capacity",
                        "capacity": -1,
                        "firstSeen": 1522941224
                    }
                ]"#,
            ))
            .expect(1)
            .mount(&server)
            .await;

        let nodes = gateway_for(&server).fetch().await.expect("fetch nodes");

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].public_key.as_str(), "node-a");
    }

    #[tokio::test]
    async fn maps_non_success_statuses() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(NODES_PATH))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&server)
            .await;

        let result = gateway_for(&server).fetch().await;

        assert_eq!(result, Err(GatewayError::HttpStatus(503)));
    }

    #[tokio::test]
    async fn maps_invalid_payloads() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(NODES_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string("not-json"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let result = gateway_for(&server).fetch().await;

        assert_eq!(result, Err(GatewayError::InvalidPayload));
    }

    #[tokio::test]
    async fn maps_connection_failures() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a free port");
        let address = listener.local_addr().expect("read the bound address");
        drop(listener);

        let gateway = MempoolNodesGateway::new(format!("http://{address}{NODES_PATH}"))
            .expect("build the mempool gateway");

        let result = gateway.fetch().await;

        assert_eq!(result, Err(GatewayError::Request));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::config::DEFAULT_MEMPOOL_URL;

    #[tokio::test]
    #[ignore = "calls the real mempool.space API"]
    async fn fetches_real_nodes_from_mempool() {
        let gateway =
            MempoolNodesGateway::new(DEFAULT_MEMPOOL_URL).expect("build the mempool gateway");

        let nodes = gateway
            .fetch()
            .await
            .expect("fetch nodes from mempool.space");

        assert!(!nodes.is_empty(), "mempool.space returned no nodes");
        assert_eq!(nodes[0].rank_position, 0);
        assert!(!nodes[0].public_key.as_str().is_empty());
        assert!(nodes[0].capacity.as_u64() > 0);
        assert!(
            nodes
                .windows(2)
                .all(|pair| pair[0].rank_position < pair[1].rank_position)
        );
    }
}
