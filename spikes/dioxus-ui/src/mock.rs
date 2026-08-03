use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MockNode {
    pub id: u32,
    pub name: String,
    pub protocol: String,
    pub region: String,
    pub latency_ms: u32,
    pub enabled: bool,
}

const PROTOCOLS: &[&str] = &[
    "Vless", "VMess", "Trojan", "Shadowsocks", "Hysteria2", "TuicV5", "NaiveProxy",
];

const REGIONS: &[&str] = &[
    "HK", "JP", "SG", "US", "DE", "UK", "KR", "TW", "CA", "FR",
];

fn pseudo_random(seed: u32) -> u32 {
    seed.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fffffff
}

pub fn generate_nodes(count: usize) -> Vec<MockNode> {
    (0..count)
        .map(|i| {
            let r = pseudo_random(i as u32);
            MockNode {
                id: i as u32,
                name: format!("Node-{i:05}"),
                protocol: PROTOCOLS[(r as usize) % PROTOCOLS.len()].to_owned(),
                region: REGIONS[(r as usize / 7) % REGIONS.len()].to_owned(),
                latency_ms: (r % 300) + 10,
                enabled: r % 3 != 0,
            }
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct TrafficPoint {
    pub day: u32,
    pub upload_gb: f64,
    pub download_gb: f64,
}

pub fn generate_traffic_data(days: u32) -> Vec<TrafficPoint> {
    (0..days)
        .map(|d| {
            let r = pseudo_random(d) as f64;
            let ratio = r / 2_147_483_647.0;
            let base = 50.0 + (d as f64 * 0.8);
            let variance = ratio * 30.0 - 15.0;
            TrafficPoint {
                day: d,
                upload_gb: (base + variance).max(0.0),
                download_gb: (base * 1.6 + variance * 1.3).max(0.0),
            }
        })
        .collect()
}

pub fn generate_group_items(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("Group-{}", i + 1)).collect()
}
