use serde_json::{map::Map, Value};
use std::{collections::HashMap, fmt};

/// That module contains an indention-block parser
mod block;

#[derive(Clone, Copy, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub enum EntryType {
    External,
    Router,
    StubNet,
    Network,
    XNetwork,
    XRouter,
}

#[derive(Clone, Copy, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub enum Metric {
    Internal(u16),
    External(u16),
}

impl fmt::Display for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Metric::Internal(x) => write!(f, "metric {}", x),
            Metric::External(x) => write!(f, "metric2 {}", x),
        }
    }
}

#[derive(Clone, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub struct Entry<'a> {
    pub typ: EntryType,
    pub obj: &'a str,
    pub metric: Metric,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum EntryParseError {
    #[error("invalid entry type")]
    InvalidEntryType,

    #[error("invalid metric value")]
    InvalidMetric(#[from] std::num::ParseIntError),

    #[error("unknown metric")]
    UnknownMetric,

    #[error("entry with invalid structure (elements = {0})")]
    InvalidStructure(usize),
}

impl std::str::FromStr for EntryType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        Ok(match s {
            "external" => EntryType::External,
            "router" => EntryType::Router,
            "stubnet" => EntryType::StubNet,
            "network" => EntryType::Network,
            "xnetwork" => EntryType::XNetwork,
            "xrouter" => EntryType::XRouter,
            _ => return Err(()),
        })
    }
}

impl Metric {
    fn new(t: &str, v: &str) -> Result<Metric, EntryParseError> {
        let v: u16 = v.parse()?;
        match t {
            "metric" => Ok(Metric::Internal(v)),
            "metric2" => Ok(Metric::External(v)),
            _ => Err(EntryParseError::UnknownMetric),
        }
    }
}

impl<'a> Entry<'a> {
    fn from_str(s: &'a str) -> Result<Self, EntryParseError> {
        let parts: Vec<_> = s.split_ascii_whitespace().collect();
        if parts.len() != 4 {
            return Err(EntryParseError::InvalidStructure(parts.len()));
        }
        Ok(Entry {
            typ: parts[0]
                .parse()
                .map_err(|()| EntryParseError::InvalidEntryType)?,
            obj: parts[1],
            metric: Metric::new(parts[2], parts[3])?,
        })
    }
}

#[derive(Clone, Debug, PartialOrd, PartialEq)]
pub struct RouterData<'a> {
    distance: u8,
    entries: Vec<Entry<'a>>,
}

impl<'a> RouterData<'a> {
    pub fn get_details(&self) -> Map<String, Value> {
        let mut ret = Map::new();
        ret.insert("distance".to_string(), Value::Number(self.distance.into()));
        for i in self.entries.iter() {
            if let Value::Array(ref mut a) = ret
                .entry(format!("{:?}", i.typ))
                .or_insert_with(|| Value::Array(Vec::new()))
            {
                a.push(Value::String(format!("{} {}", i.obj, i.metric)));
            }
        }
        ret
    }
    pub fn neighbors(&self) -> Vec<(&'a str, u16)> {
        self.entries
            .iter()
            .filter_map(|i| {
                if i.typ == EntryType::Router {
                    Some((
                        i.obj,
                        match i.metric {
                            Metric::Internal(x) => x,
                            Metric::External(x) => 1000 + x,
                        },
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn conns(&self) -> Vec<(&'a str, u16)> {
        self.entries
            .iter()
            .filter_map(|i| {
                if i.typ != EntryType::Router {
                    Some((
                        i.obj,
                        match i.metric {
                            Metric::Internal(x) => x,
                            Metric::External(x) => 1000 + x,
                        },
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn is_unreachable(&self) -> bool {
        self.distance == 255
    }
}

pub struct Topology<'a> {
    pub routers: HashMap<u64, &'a str>,
    pub areas: HashMap<&'a str, HashMap<u64, RouterData<'a>>>,
}

impl Topology<'_> {
    pub fn new() -> Topology<'static> {
        Topology {
            routers: HashMap::new(),
            areas: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum TopologyParseError<'a> {
    #[error("invalid entry ({err}): {ent}")]
    InvalidEntry { ent: &'a str, err: EntryParseError },

    #[error("invalid distance value")]
    InvalidDistance(#[from] std::num::ParseIntError),

    #[error("unknown topology structure (level {0})")]
    UnknownStructure(u32),

    #[error("attempt to merge topologies with mismatching distance values (old = {0}, new = {1})")]
    DistanceMismatch(u8, u8),
}

pub fn router2id(router: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    router.hash(&mut hasher);
    let h = hasher.finish();
    h
}

pub fn parse_topology<'a, 'b: 'a>(
    base_topo: Topology<'b>,
    s: &'a str,
) -> Result<Topology<'a>, TopologyParseError<'a>> {
    static AREA_PFX: &str = "area ";
    static ROUTER_PFX: &str = "router ";
    static DISTANCE_PFX: &str = "distance ";

    let mut blocks_ = block::parse_nested_blocks(s);
    if blocks_.is_empty() || !blocks_.remove(0).head.starts_with("BIRD v") {
        return Err(TopologyParseError::UnknownStructure(0));
    }

    let Topology {
        mut routers,
        mut areas,
    } = base_topo;
    let mut rrcache = HashMap::new();
    let mut register_router = |router: &'a str| -> u64 {
        *rrcache.entry(router).or_insert_with(|| {
            let h = router2id(router);
            routers.insert(h, router);
            h
        })
    };

    for area in blocks_ {
        if !area.head.starts_with(AREA_PFX) {
            return Err(TopologyParseError::UnknownStructure(1));
        }
        let area_name = &area.head[AREA_PFX.len()..];
        let areadat = areas.entry(area_name).or_insert_with(HashMap::new);

        for router in &area.subs {
            if !router.head.starts_with(ROUTER_PFX) {
                return Err(TopologyParseError::UnknownStructure(2));
            }
            let router_name = &router.head[ROUTER_PFX.len()..];
            let rid = register_router(router_name);
            let mut rdat = areadat.entry(rid).or_insert_with(|| RouterData {
                distance: 255,
                entries: Vec::new(),
            });

            for ent in &router.subs {
                if !ent.subs.is_empty() {
                    return Err(TopologyParseError::UnknownStructure(3));
                }
                if ent.head == "unreachable" {
                    let new_distance: u8 = 255;
                    if rdat.distance != new_distance && rdat.distance != 255 {
                        return Err(TopologyParseError::DistanceMismatch(
                            rdat.distance,
                            new_distance,
                        ));
                    }
                    rdat.distance = new_distance;
                } else if ent.head.starts_with(DISTANCE_PFX) {
                    let new_distance: u8 = ent.head[DISTANCE_PFX.len()..].parse()?;
                    if rdat.distance != new_distance && rdat.distance != 255 {
                        return Err(TopologyParseError::DistanceMismatch(
                            rdat.distance,
                            new_distance,
                        ));
                    }
                    rdat.distance = new_distance;
                } else {
                    rdat.entries.push(
                        Entry::from_str(ent.head).map_err(|err| {
                            TopologyParseError::InvalidEntry { ent: ent.head, err }
                        })?,
                    );
                }
            }
            rdat.entries.sort();
            rdat.entries.dedup();
        }
    }

    Ok(Topology { routers, areas })
}
