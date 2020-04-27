use serde::Serialize;
use serde_json::{map::Map, Value};
use std::collections::HashMap;
use tracing::error;

#[derive(Serialize)]
struct Node {
    id: u64,
    label: String,
    group: String,
    details: Map<String, Value>,
}

#[derive(Serialize, PartialOrd, PartialEq, Ord, Eq)]
struct Edge {
    from: u64,
    to: u64,
}

pub fn gather(protos: &[&str]) -> Option<String> {
    let mut tmp = Vec::new();
    for i in protos.iter().copied() {
        let outp = match std::process::Command::new("birdc")
            .args(&["show", "ospf", "state", "all", i])
            .output()
        {
            Ok(outp) => outp,
            Err(x) => {
                error!("gather: run birdc[{}] failed: {:?}", i, x);
                continue;
            }
        };
        if !outp.status.success() {
            error!(
                "gather: run birdc[{}] failed:\n{}",
                i,
                String::from_utf8_lossy(&outp.stderr[..])
            );
            continue;
        }
        tmp.push(String::from_utf8(outp.stdout).expect("got non-utf8 birdc output"));
    }
    let mut topo = crate::parser::Topology::new();
    for i in tmp.iter() {
        topo = match crate::parser::parse_topology(topo, i) {
            Ok(topo) => topo,
            Err(x) => {
                error!("gather: parsing birdc output failed ({}):\n{}", x, i);
                return None;
            }
        };
    }
    if topo.areas.is_empty() {
        return None;
    }
    let mut routers: HashMap<u64, (bool, Map<String, Value>)> =
        topo.routers.iter().map(|(&k, _)| (k, (false, Map::new()))).collect();
    let mut edges: Vec<Edge> = Vec::new();
    if let Some(bb_area) = topo.areas.get("0.0.0.0") {
        for (&rid, router) in bb_area.iter() {
            routers.insert(rid, (!router.is_unreachable(), router.get_details()));
            for i in router.neighbors() {
                let orid = crate::parser::router2id(i);
                edges.push(Edge {
                    from: std::cmp::min(rid, orid),
                    to: std::cmp::max(rid, orid),
                });
            }
        }
    }
    let nodes: Vec<Node> = topo
        .routers
        .iter()
        .map(|(&k, &v)| Node {
            id: k,
            label: v.to_string(),
            group: if routers.get(&k).map(|i| i.0).unwrap_or(false) { "ytrizja" } else { "unreachable" }.to_string(),
            details: routers.get(&k).cloned().map(|i| i.1).unwrap_or_else(Map::new),
        })
        .collect();
    edges.sort();
    edges.dedup();

    let mut ret = Map::new();
    ret.insert("nodes".to_string(), serde_json::to_value(&nodes).expect("unable to serialize nodes"));
    ret.insert("edges".to_string(), serde_json::to_value(&edges).expect("unable to serialize edges"));
    Some(serde_json::to_string(&ret).expect("unable to serialize data"))
}
