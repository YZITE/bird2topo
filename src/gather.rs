use serde::Serialize;
use serde_json::{map::Map, Value};
use std::collections::HashMap;
use tracing::error;

#[derive(Clone, Serialize)]
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
    length: u16,
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
    let mut nodes_: HashMap<u64, (&str, bool, Map<String, Value>)> = topo
        .interned
        .iter()
        .map(|(&k, &v)| (k, (v, false, Map::new())))
        .collect();
    let mut nodes: HashMap<u64, Node> = HashMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    if let Some(bb_area) = topo.areas.get("0.0.0.0") {
        let mut insert_edge = |id1, id2, w| {
            edges.push(Edge {
                from: std::cmp::min(id1, id2),
                to: std::cmp::max(id1, id2),
                length: std::cmp::min(w / 100 + 1, 1000),
            });
        };
        for (&rid, router) in bb_area.routers.iter() {
            let mut roun = nodes_.get_mut(&rid).unwrap();
            roun.1 = !router.is_unreachable();
            roun.2 = router.get_details();
            for (i, w) in router.neighbors() {
                let orid = crate::parser::router2id(i);
                insert_edge(rid, orid, w);
            }
            for (i, w) in router.conns() {
                let orid = crate::parser::router2id(i);
                nodes.entry(orid).or_insert_with(|| Node {
                    id: orid,
                    label: i.to_string(),
                    group: "network".to_string(),
                    details: Map::new(),
                });
                insert_edge(rid, orid, w);
            }
        }
        for (&nid, network) in bb_area.networks.iter() {
            let mut ntwn = nodes_.get_mut(&nid).unwrap();
            ntwn.1 = !network.is_unreachable();
            ntwn.2.insert(
                "distance".to_string(),
                Value::Number(network.distance.into()),
            );
            for i in network
                .routers
                .iter()
                .copied()
                .chain(std::iter::once(network.dr))
            {
                insert_edge(nid, i, 0);
            }
        }
    }
    nodes.extend(nodes_.iter().map(|(&k, v)| {
        (
            k,
            Node {
                id: k,
                label: v.0.to_string(),
                group: if !v.1 {
                    "unreachable"
                } else if v.0.contains('/') {
                    "network"
                } else {
                    "ytrizja"
                }
                .to_string(),
                details: v.2.clone(),
            },
        )
    }));
    edges.sort();
    edges.dedup();

    let nodes: Vec<Node> = nodes.values().cloned().collect();
    let mut ret = Map::new();
    ret.insert(
        "nodes".to_string(),
        serde_json::to_value(&nodes).expect("unable to serialize nodes"),
    );
    ret.insert(
        "edges".to_string(),
        serde_json::to_value(&edges).expect("unable to serialize edges"),
    );
    Some(serde_json::to_string(&ret).expect("unable to serialize data"))
}
