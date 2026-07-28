//! The mesh manifest: one file describing every node, from which each node's
//! `wg` config is generated as "all peers minus itself".
//!
//! Public data (name, address, endpoint, public key) is safe to share/commit.
//! Private keys are optional here so you can keep a public-only manifest and
//! inject the private key at generation time; when present, it is written into
//! that node's `[Interface]`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
pub struct Manifest {
    #[serde(default = "default_port")]
    pub listen_port: u16,
    #[serde(rename = "node", default)]
    pub nodes: Vec<Node>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Node {
    pub name: String,
    /// Interface address with prefix, e.g. `10.10.0.1/24`.
    pub address: String,
    pub public_key: String,
    /// Where peers reach this node (`host:port`). Nodes others dial need one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// What peers route TO this node. Defaults to just this node's `/32`; set it
    /// to `0.0.0.0/0` (a full-tunnel exit) or a LAN subnet (site-to-site) so peers
    /// send that traffic here. Comma-separated for multiple.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_ips: Option<String>,
    /// DNS server for THIS node's own `[Interface]` (e.g. a Pi-hole behind the
    /// tunnel), written as `DNS = ...` when present. Comma-separated for multiple.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns: Option<String>,
    /// Seconds between keepalives sent TO this node (set on a stable/behind-NAT
    /// endpoint so roaming peers hold the tunnel open, e.g. `25`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keepalive: Option<u16>,
    /// For a SPOKE (no endpoint): the hubs it connects to, by name. Empty = all
    /// hubs. A HUB (has an endpoint) ignores this and peers with everyone that
    /// needs it. This is what makes the mesh hub-and-spoke instead of full.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hubs: Vec<String>,
    /// Written to this node's own `[Interface]` when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_up: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_down: Option<String>,
}

fn default_port() -> u16 {
    51820
}

impl Manifest {
    /// Load and validate a manifest (fails if empty). Use for `gen`.
    pub fn load(path: &Path) -> Result<Self> {
        let manifest = Self::read(path)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Load without the non-empty requirement, or a fresh empty manifest if the
    /// file does not exist. Use for `node` editing.
    pub fn load_or_empty(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .with_context(|| format!("parsing manifest `{}`", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest {
                listen_port: default_port(),
                nodes: Vec::new(),
            }),
            Err(e) => Err(e).with_context(|| format!("reading manifest `{}`", path.display())),
        }
    }

    fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest `{}`", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing manifest `{}`", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string(self).context("serializing manifest")?;
        std::fs::write(path, text).with_context(|| format!("writing manifest `{}`", path.display()))
    }

    /// Append a node, rejecting duplicate names or mesh addresses.
    pub fn add(&mut self, node: Node) -> Result<()> {
        if self.nodes.iter().any(|n| n.name == node.name) {
            bail!("node `{}` already exists", node.name);
        }
        let ip = ip_of(&node.address).to_string();
        if self.nodes.iter().any(|n| ip_of(&n.address) == ip) {
            bail!("mesh address `{ip}` already in use");
        }
        self.nodes.push(node);
        Ok(())
    }

    /// Remove a node by name.
    pub fn remove(&mut self, name: &str) -> Result<()> {
        let before = self.nodes.len();
        self.nodes.retain(|n| n.name != name);
        if self.nodes.len() == before {
            bail!("no node named `{name}`");
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.nodes.is_empty() {
            bail!("manifest has no nodes - add [[node]] entries");
        }
        let mut names = HashSet::new();
        let mut addrs = HashSet::new();
        for n in &self.nodes {
            if !names.insert(&n.name) {
                bail!("duplicate node name `{}` - names must be unique", n.name);
            }
            let ip = ip_of(&n.address);
            if !addrs.insert(ip.to_string()) {
                bail!("duplicate mesh address `{ip}` - each node needs its own");
            }
        }
        Ok(())
    }

    /// Whether `node` is a hub (reachable: has an endpoint) vs a spoke.
    fn is_hub(node: &Node) -> bool {
        node.endpoint.is_some()
    }

    /// Should `node`'s config list `peer`? Hub-and-spoke rules:
    /// - a hub meshes with every other hub, and with spokes that point at it
    ///   (or point nowhere, meaning "all hubs");
    /// - a spoke lists only its hubs (or every hub when it named none).
    fn peers_with(&self, node: &Node, peer: &Node) -> bool {
        if peer.name == node.name {
            return false;
        }
        if Self::is_hub(node) {
            Self::is_hub(peer) || peer.hubs.is_empty() || peer.hubs.iter().any(|h| h == &node.name)
        } else {
            Self::is_hub(peer) && (node.hubs.is_empty() || node.hubs.iter().any(|h| h == &peer.name))
        }
    }

    /// Generate `node`'s config: its `[Interface]` plus a `[Peer]` for each node
    /// it should reach under the hub-and-spoke rules, routed by allowed-ips.
    pub fn node_config(&self, node: &Node) -> String {
        let mut s = String::new();
        s.push_str("[Interface]\n");
        match &node.private_key {
            Some(pk) => s.push_str(&format!("PrivateKey = {pk}\n")),
            None => s.push_str("PrivateKey = <PASTE PRIVATE KEY>\n"),
        }
        s.push_str(&format!("Address    = {}\n", node.address));
        if let Some(dns) = &node.dns {
            s.push_str(&format!("DNS        = {dns}\n"));
        }
        s.push_str(&format!("ListenPort = {}\n", self.listen_port));
        if let Some(pu) = &node.post_up {
            s.push_str(&format!("PostUp     = {pu}\n"));
        }
        if let Some(pd) = &node.post_down {
            s.push_str(&format!("PostDown   = {pd}\n"));
        }

        for peer in &self.nodes {
            if !self.peers_with(node, peer) {
                continue;
            }
            s.push_str(&format!("\n[Peer]  # {}\n", peer.name));
            s.push_str(&format!("PublicKey  = {}\n", peer.public_key));
            if let Some(ep) = &peer.endpoint {
                s.push_str(&format!("Endpoint   = {ep}\n"));
            }
            // What this peer advertises: its override (e.g. 0.0.0.0/0 for a
            // full-tunnel exit, or a LAN subnet) or just its own /32 by default.
            let ips = peer
                .allowed_ips
                .clone()
                .unwrap_or_else(|| format!("{}/32", ip_of(&peer.address)));
            s.push_str(&format!("AllowedIPs = {ips}\n"));
            if let Some(k) = peer.keepalive {
                s.push_str(&format!("PersistentKeepalive = {k}\n"));
            }
        }
        s
    }
}

impl Node {
    /// The bare mesh IP (address without its prefix).
    pub fn mesh_ip(&self) -> &str {
        ip_of(&self.address)
    }
}

/// The bare IP from an `address` field: `10.10.0.1/24` -> `10.10.0.1`.
fn ip_of(address: &str) -> &str {
    address.split('/').next().unwrap_or(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            listen_port: 51820,
            nodes: vec![
                Node {
                    name: "A".into(),
                    address: "10.10.0.1/24".into(),
                    public_key: "PUB_A".into(),
                    endpoint: Some("vpn-a.example.com:51820".into()),
                    private_key: Some("PRIV_A".into()),
                    allowed_ips: None,
                    dns: None,
                    keepalive: None,
                    hubs: Vec::new(),
                    post_up: Some("iptables -A FORWARD -i wg0 -j ACCEPT".into()),
                    post_down: None,
                },
                Node {
                    name: "B".into(),
                    address: "10.10.0.2/24".into(),
                    public_key: "PUB_B".into(),
                    endpoint: Some("vpn-b.example.com:51820".into()),
                    private_key: None,
                    allowed_ips: None,
                    dns: None,
                    keepalive: None,
                    hubs: Vec::new(),
                    post_up: None,
                    post_down: None,
                },
            ],
        }
    }

    #[test]
    fn a_node_never_lists_itself_as_a_peer() {
        let m = manifest();
        let cfg = m.node_config(&m.nodes[0]);
        assert!(
            !cfg.contains("PUB_A"),
            "own pubkey must not appear as a peer"
        );
        assert!(cfg.contains("PUB_B"), "the other node must be a peer");
    }

    #[test]
    fn peer_allowed_ips_are_slash_32() {
        let m = manifest();
        let cfg = m.node_config(&m.nodes[0]);
        assert!(cfg.contains("AllowedIPs = 10.10.0.2/32"));
        assert!(!cfg.contains("10.10.0.2/24"), "peers are single /32 routes");
    }

    #[test]
    fn overrides_emit_full_tunnel_dns_and_keepalive() {
        let mut m = manifest();
        m.nodes[0].allowed_ips = Some("0.0.0.0/0".into()); // A is a full-tunnel exit
        m.nodes[0].keepalive = Some(25);
        m.nodes[1].dns = Some("192.168.10.250".into()); // B uses a Pi-hole
        let cfg_b = m.node_config(&m.nodes[1]);
        assert!(cfg_b.contains("AllowedIPs = 0.0.0.0/0"), "A's override should win over /32");
        assert!(cfg_b.contains("PersistentKeepalive = 25"));
        assert!(cfg_b.contains("DNS        = 192.168.10.250"), "B's own interface DNS");
        // and the default is still a /32 when unset (A's config, B has no override)
        assert!(m.node_config(&m.nodes[0]).contains("AllowedIPs = 10.10.0.2/32"));
    }

    #[test]
    fn interface_carries_private_key_and_postup_when_present() {
        let m = manifest();
        let cfg = m.node_config(&m.nodes[0]);
        assert!(cfg.contains("PrivateKey = PRIV_A"));
        assert!(cfg.contains("PostUp     = iptables"));
    }

    #[test]
    fn missing_private_key_leaves_a_placeholder_not_a_crash() {
        let m = manifest();
        let cfg = m.node_config(&m.nodes[1]);
        assert!(cfg.contains("PrivateKey = <PASTE PRIVATE KEY>"));
    }

    #[test]
    fn duplicate_names_are_rejected() {
        let m = Manifest {
            listen_port: 51820,
            nodes: vec![
                Node {
                    name: "A".into(),
                    address: "10.10.0.1/24".into(),
                    public_key: "x".into(),
                    endpoint: None,
                    private_key: None,
                    allowed_ips: None,
                    dns: None,
                    keepalive: None,
                    hubs: Vec::new(),
                    post_up: None,
                    post_down: None,
                },
                Node {
                    name: "A".into(),
                    address: "10.10.0.2/24".into(),
                    public_key: "y".into(),
                    endpoint: None,
                    private_key: None,
                    allowed_ips: None,
                    dns: None,
                    keepalive: None,
                    hubs: Vec::new(),
                    post_up: None,
                    post_down: None,
                },
            ],
        };
        let e = m.validate().unwrap_err().to_string();
        assert!(e.contains("duplicate node name"), "got: {e}");
    }

    #[test]
    fn add_rejects_duplicate_name_and_address() {
        let mut m = manifest();
        let dup_name = Node {
            name: "A".into(),
            address: "10.10.0.9/24".into(),
            public_key: "z".into(),
            endpoint: None,
            private_key: None,
            allowed_ips: None,
            dns: None,
            keepalive: None,
            hubs: Vec::new(),
            post_up: None,
            post_down: None,
        };
        assert!(
            m.add(dup_name)
                .unwrap_err()
                .to_string()
                .contains("already exists")
        );

        let dup_addr = Node {
            name: "Z".into(),
            address: "10.10.0.1/24".into(),
            public_key: "z".into(),
            endpoint: None,
            private_key: None,
            allowed_ips: None,
            dns: None,
            keepalive: None,
            hubs: Vec::new(),
            post_up: None,
            post_down: None,
        };
        assert!(
            m.add(dup_addr)
                .unwrap_err()
                .to_string()
                .contains("already in use")
        );
    }

    #[test]
    fn remove_unknown_node_errors() {
        let mut m = manifest();
        assert!(
            m.remove("nope")
                .unwrap_err()
                .to_string()
                .contains("no node named")
        );
        assert!(m.remove("A").is_ok());
    }

    #[test]
    fn save_load_roundtrip_preserves_nodes_and_omits_empty_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mesh.toml");
        manifest().save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("post_down"), "None fields must be omitted");

        let back = Manifest::load_or_empty(&path).unwrap();
        assert_eq!(back.nodes.len(), 2);
        assert_eq!(back.nodes[0].name, "A");
    }

    #[test]
    fn duplicate_addresses_are_rejected() {
        let m = Manifest {
            listen_port: 51820,
            nodes: vec![
                Node {
                    name: "A".into(),
                    address: "10.10.0.1/24".into(),
                    public_key: "x".into(),
                    endpoint: None,
                    private_key: None,
                    allowed_ips: None,
                    dns: None,
                    keepalive: None,
                    hubs: Vec::new(),
                    post_up: None,
                    post_down: None,
                },
                Node {
                    name: "B".into(),
                    address: "10.10.0.1/24".into(),
                    public_key: "y".into(),
                    endpoint: None,
                    private_key: None,
                    allowed_ips: None,
                    dns: None,
                    keepalive: None,
                    hubs: Vec::new(),
                    post_up: None,
                    post_down: None,
                },
            ],
        };
        let e = m.validate().unwrap_err().to_string();
        assert!(e.contains("duplicate mesh address"), "got: {e}");
    }
}
