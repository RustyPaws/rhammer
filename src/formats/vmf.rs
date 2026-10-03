//! VMF document model. Anything we do not understand is stored as raw KeyValues
//! nodes and written back untouched, so files stay compatible with Hammer / Hammer++.

use crate::kv::{self as kv, Node, NodeList, Value};
use anyhow::{Context, Result};
use glam::DVec3;

#[derive(Clone, Debug, PartialEq)]
pub struct TexAxis {
    pub vec: DVec3,
    pub shift: f64,
    pub scale: f64,
}

impl TexAxis {
    pub fn parse(s: &str) -> TexAxis {
        // "[1 0 0 0] 0.25"
        let cleaned = s.replace(['[', ']'], " ");
        let nums: Vec<f64> = cleaned.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        let g = |i: usize, d: f64| nums.get(i).copied().unwrap_or(d);
        TexAxis { vec: DVec3::new(g(0, 0.0), g(1, 0.0), g(2, 0.0)), shift: g(3, 0.0), scale: g(4, 0.25) }
    }
    pub fn to_string(&self) -> String {
        format!("[{} {} {} {}] {}", fmt(self.vec.x), fmt(self.vec.y), fmt(self.vec.z), fmt(self.shift), fmt(self.scale))
    }
}

#[derive(Clone, Debug)]
pub struct Side {
    pub id: u32,
    pub plane: [DVec3; 3],
    pub material: String,
    pub uaxis: TexAxis,
    pub vaxis: TexAxis,
    pub rotation: f64,
    pub lightmap: i32,
    pub smoothing: u32,
    /// Raw `dispinfo` block children, if this side is a displacement.
    pub dispinfo: Option<Vec<Node>>,
    /// Unknown child nodes (kept verbatim).
    pub extra: Vec<Node>,
}

#[derive(Clone, Debug, Default)]
pub struct Solid {
    pub id: u32,
    pub sides: Vec<Side>,
    pub editor: Vec<Node>,
    pub hidden: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Entity {
    pub id: u32,
    /// Ordered key/value pairs, excluding `id`.
    pub props: Vec<(String, String)>,
    /// Entity I/O: the `connections` block.
    pub connections: Vec<Connection>,
    pub solids: Vec<Solid>,
    pub editor: Vec<Node>,
    pub hidden: bool,
    pub extra: Vec<Node>,
}

/// Parsed form of one `connections` entry. Newer branches (Portal 2, L4D2, CS:GO) separate
/// the fields with ESC instead of ','.
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub output: String,
    pub target: String,
    pub input: String,
    pub param: String,
    pub delay: f64,
    /// Times to fire; -1 means unlimited.
    pub times: i32,
    pub sep: char,
}

impl Connection {
    /// Fresh `OnTrigger -> Trigger` connection using the given field separator.
    pub fn new(sep: char) -> Connection {
        Connection { output: "OnTrigger".into(), target: String::new(), input: "Trigger".into(), param: String::new(), delay: 0.0, times: -1, sep }
    }

    pub fn parse((output, value): &(String, String)) -> Connection {
        let sep = if value.contains('\x1b') { '\x1b' } else { ',' };
        let mut it = value.splitn(5, sep);
        let mut next = || it.next().unwrap_or("").to_string();
        let target = next();
        let input = next();
        let param = next();
        let delay = next().trim().parse().unwrap_or(0.0);
        let times = next().trim().parse().unwrap_or(-1);
        Connection { output: output.clone(), target, input, param, delay, times, sep }
    }

    pub fn to_pair(&self) -> (String, String) {
        let s = self.sep;
        let v = format!("{}{s}{}{s}{}{s}{}{s}{}", self.target, self.input, self.param, fmt(self.delay), self.times);
        (self.output.clone(), v)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Map {
    /// Top level nodes before `world` (versioninfo, visgroups, viewsettings, *_plus, ...).
    pub header: Vec<Node>,
    pub world: Entity,
    pub entities: Vec<Entity>,
    /// Top level nodes after entities (cameras, cordons, ...).
    pub footer: Vec<Node>,
}

pub fn fmt(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{:.6}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub fn parse_vec3(s: &str) -> Option<DVec3> {
    let cleaned = s.replace(['(', ')', '[', ']'], " ");
    let mut it = cleaned.split_whitespace().map(|t| t.parse::<f64>());
    Some(DVec3::new(it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?))
}

pub fn fmt_vec3(v: DVec3) -> String {
    format!("{} {} {}", fmt(v.x), fmt(v.y), fmt(v.z))
}

fn parse_plane(s: &str) -> [DVec3; 3] {
    // "(x y z) (x y z) (x y z)"
    let mut pts = [DVec3::ZERO; 3];
    for (i, part) in s.split(')').filter(|p| p.contains('(')).take(3).enumerate() {
        if let Some(v) = parse_vec3(part) {
            pts[i] = v;
        }
    }
    pts
}

fn plane_string(p: &[DVec3; 3]) -> String {
    format!("({}) ({}) ({})", fmt_vec3(p[0]), fmt_vec3(p[1]), fmt_vec3(p[2]))
}

impl Side {
    fn from_nodes(nodes: &[Node]) -> Side {
        let mut s = Side {
            id: 0,
            plane: [DVec3::ZERO; 3],
            material: "DEV/DEV_MEASUREGENERIC01B".into(),
            uaxis: TexAxis { vec: DVec3::X, shift: 0.0, scale: 0.25 },
            vaxis: TexAxis { vec: DVec3::NEG_Y, shift: 0.0, scale: 0.25 },
            rotation: 0.0,
            lightmap: 16,
            smoothing: 0,
            dispinfo: None,
            extra: Vec::new(),
        };
        for n in nodes {
            match (&n.key.to_ascii_lowercase()[..], &n.value) {
                ("id", Value::Str(v)) => s.id = v.parse().unwrap_or(0),
                ("plane", Value::Str(v)) => s.plane = parse_plane(v),
                ("material", Value::Str(v)) => s.material = v.clone(),
                ("uaxis", Value::Str(v)) => s.uaxis = TexAxis::parse(v),
                ("vaxis", Value::Str(v)) => s.vaxis = TexAxis::parse(v),
                ("rotation", Value::Str(v)) => s.rotation = v.parse().unwrap_or(0.0),
                ("lightmapscale", Value::Str(v)) => s.lightmap = v.parse().unwrap_or(16),
                ("smoothing_groups", Value::Str(v)) => s.smoothing = v.parse().unwrap_or(0),
                ("dispinfo", Value::Block(c)) => s.dispinfo = Some(c.clone()),
                // Recomputed from the planes; dropping avoids stale data after edits.
                ("vertices_plus", _) => {}
                _ => s.extra.push(n.clone()),
            }
        }
        s
    }

    fn to_node(&self) -> Node {
        let mut c = vec![
            Node::str("id", self.id.to_string()),
            Node::str("plane", plane_string(&self.plane)),
            Node::str("material", self.material.clone()),
            Node::str("uaxis", self.uaxis.to_string()),
            Node::str("vaxis", self.vaxis.to_string()),
            Node::str("rotation", fmt(self.rotation)),
            Node::str("lightmapscale", self.lightmap.to_string()),
            Node::str("smoothing_groups", self.smoothing.to_string()),
        ];
        if let Some(d) = &self.dispinfo {
            c.push(Node::block("dispinfo", d.clone()));
        }
        c.extend(self.extra.iter().cloned());
        Node::block("side", c)
    }
}

impl Solid {
    fn from_nodes(nodes: &[Node], hidden: bool) -> Solid {
        let mut s = Solid { hidden, ..Default::default() };
        for n in nodes {
            match (&n.key.to_ascii_lowercase()[..], &n.value) {
                ("id", Value::Str(v)) => s.id = v.parse().unwrap_or(0),
                ("side", Value::Block(c)) => s.sides.push(Side::from_nodes(c)),
                ("editor", Value::Block(c)) => s.editor = c.clone(),
                _ => {}
            }
        }
        s
    }

    fn to_node(&self) -> Node {
        let mut c = vec![Node::str("id", self.id.to_string())];
        c.extend(self.sides.iter().map(|s| s.to_node()));
        c.push(Node::block("editor", self.editor.clone()));
        Node::block("solid", c)
    }
}

impl Entity {
    fn from_nodes(nodes: &[Node], hidden: bool) -> Entity {
        let mut e = Entity { hidden, ..Default::default() };
        for n in nodes {
            let k = n.key.to_ascii_lowercase();
            match (&k[..], &n.value) {
                ("id", Value::Str(v)) => e.id = v.parse().unwrap_or(0),
                ("solid", Value::Block(c)) => e.solids.push(Solid::from_nodes(c, hidden)),
                ("connections", Value::Block(c)) => {
                    for cn in c {
                        if let Some(v) = cn.as_str() {
                            e.connections.push(Connection::parse(&(cn.key.clone(), v.to_string())));
                        }
                    }
                }
                ("editor", Value::Block(c)) => e.editor = c.clone(),
                ("hidden", Value::Block(c)) => {
                    for h in c {
                        if h.key.eq_ignore_ascii_case("solid") {
                            e.solids.push(Solid::from_nodes(h.children(), true));
                        }
                    }
                }
                (_, Value::Str(v)) => e.props.push((n.key.clone(), v.clone())),
                _ => e.extra.push(n.clone()),
            }
        }
        e
    }

    fn to_nodes(&self) -> Vec<Node> {
        let mut c = vec![Node::str("id", self.id.to_string())];
        for (k, v) in &self.props {
            c.push(Node::str(k.clone(), v.clone()));
        }
        if !self.connections.is_empty() {
            c.push(Node::block(
                "connections",
                self.connections
                    .iter()
                    .map(|c| {
                        let (k, v) = c.to_pair();
                        Node::str(k, v)
                    })
                    .collect(),
            ));
        }
        for s in self.solids.iter().filter(|s| !s.hidden) {
            c.push(s.to_node());
        }
        let hid: Vec<Node> = self.solids.iter().filter(|s| s.hidden).map(|s| s.to_node()).collect();
        if !hid.is_empty() && !self.hidden {
            c.push(Node::block("hidden", hid));
        }
        c.extend(self.extra.iter().cloned());
        if !self.editor.is_empty() {
            c.push(Node::block("editor", self.editor.clone()));
        }
        c
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }

    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or("")
    }

    pub fn set(&mut self, key: &str, val: impl Into<String>) {
        let val = val.into();
        if let Some(p) = self.props.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            p.1 = val;
        } else {
            self.props.push((key.to_string(), val));
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.props.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    }

    pub fn origin(&self) -> DVec3 {
        self.get("origin").and_then(parse_vec3).unwrap_or(DVec3::ZERO)
    }

    pub fn set_origin(&mut self, o: DVec3) {
        self.set("origin", fmt_vec3(o));
    }

    pub fn angles(&self) -> DVec3 {
        self.get("angles").and_then(parse_vec3).unwrap_or(DVec3::ZERO)
    }

    pub fn editor_str(&self, key: &str) -> Option<&str> {
        self.editor.get_str(key)
    }
}

impl Map {
    pub fn parse(text: &str) -> Result<Map> {
        let nodes = kv::parse(text).context("parsing VMF")?;
        let mut m = Map::default();
        let mut seen_world = false;
        for n in nodes {
            let k = n.key.to_ascii_lowercase();
            match (&k[..], &n.value) {
                ("world", Value::Block(c)) => {
                    m.world = Entity::from_nodes(c, false);
                    seen_world = true;
                }
                ("entity", Value::Block(c)) => m.entities.push(Entity::from_nodes(c, false)),
                ("hidden", Value::Block(c)) => {
                    for h in c {
                        if h.key.eq_ignore_ascii_case("entity") {
                            m.entities.push(Entity::from_nodes(h.children(), true));
                        }
                    }
                }
                _ => {
                    if seen_world {
                        m.footer.push(n)
                    } else {
                        m.header.push(n)
                    }
                }
            }
        }
        Ok(m)
    }

    pub fn load(path: &std::path::Path) -> Result<Map> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Map::parse(&String::from_utf8_lossy(&bytes))
    }

    pub fn to_text(&self) -> String {
        let mut nodes: Vec<Node> = self.header.clone();
        nodes.push(Node::block("world", self.world.to_nodes()));
        for e in self.entities.iter().filter(|e| !e.hidden) {
            nodes.push(Node::block("entity", e.to_nodes()));
        }
        let hid: Vec<Node> = self
            .entities
            .iter()
            .filter(|e| e.hidden)
            .map(|e| Node::block("entity", e.to_nodes()))
            .collect();
        if !hid.is_empty() {
            nodes.push(Node::block("hidden", hid));
        }
        nodes.extend(self.footer.iter().cloned());
        kv::to_string(&nodes)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        std::fs::write(path, self.to_text()).with_context(|| format!("writing {}", path.display()))
    }

    /// An empty, valid map.
    pub fn new_empty() -> Map {
        let mut m = Map::default();
        m.header = vec![
            Node::block(
                "versioninfo",
                vec![
                    Node::str("editorversion", "400"),
                    Node::str("editorbuild", "8869"),
                    Node::str("mapversion", "1"),
                    Node::str("formatversion", "100"),
                    Node::str("prefab", "0"),
                ],
            ),
            Node::block("visgroups", vec![]),
            Node::block(
                "viewsettings",
                vec![
                    Node::str("bSnapToGrid", "1"),
                    Node::str("bShowGrid", "1"),
                    Node::str("bShowLogicalGrid", "0"),
                    Node::str("nGridSpacing", "16"),
                    Node::str("bShow3DGrid", "0"),
                ],
            ),
        ];
        m.world = Entity { id: 1, ..Default::default() };
        m.world.props = vec![
            ("mapversion".into(), "1".into()),
            ("classname".into(), "worldspawn".into()),
            ("skyname".into(), "sky_day01_01".into()),
        ];
        m.footer = vec![
            Node::block("cameras", vec![Node::str("activecamera", "-1")]),
            Node::block("cordons", vec![Node::str("active", "0")]),
        ];
        m
    }

    /// Highest id used by any solid, side or entity.
    pub fn max_id(&self) -> u32 {
        let mut mx = self.world.id;
        let mut scan = |e: &Entity| {
            mx = mx.max(e.id);
            for s in &e.solids {
                mx = mx.max(s.id);
                for sd in &s.sides {
                    mx = mx.max(sd.id);
                }
            }
        };
        scan(&self.world);
        for e in &self.entities {
            scan(e);
        }
        mx
    }

    pub fn grid_spacing(&self) -> f64 {
        self.header
            .get_block("viewsettings")
            .and_then(|v| v.get_str("nGridSpacing"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(16.0)
    }

    pub fn set_viewsetting(&mut self, key: &str, val: &str) {
        for n in &mut self.header {
            if n.key.eq_ignore_ascii_case("viewsettings") {
                if let Value::Block(c) = &mut n.value {
                    if let Some(x) = c.iter_mut().find(|x| x.key.eq_ignore_ascii_case(key)) {
                        x.value = Value::Str(val.to_string());
                    } else {
                        c.push(Node::str(key, val));
                    }
                }
            }
        }
    }
}
