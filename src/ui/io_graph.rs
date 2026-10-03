//! Blueprint-style visualisation of entity I/O: which entity fires which, and through what.
//! Read-only; built on `egui-snarl` from `doc.map.entities` connections.

use std::collections::{BTreeSet, HashMap, VecDeque};

use eframe::egui::{self, Color32, Pos2, RichText};
use egui_snarl::ui::{PinInfo, SnarlViewer, SnarlWidget};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};

use crate::formats::vmf::{Connection, Entity, Map};

const COL_W: f32 = 280.0;
const GAP_Y: f32 = 36.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Scope {
    /// Connected chain around the current selection.
    Selection,
    /// Every entity that has or receives a connection.
    Map,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Key {
    Ent(u32),
    /// Target name that no entity carries.
    Ghost(String),
}

pub struct IoNode {
    key: Key,
    title: String,
    outputs: Vec<String>,
    inputs: Vec<String>,
}

/// One resolved edge of the graph.
struct Edge {
    from: u32,
    output: String,
    to: Key,
    input: String,
    note: String,
}

pub struct IoGraph {
    pub scope: Scope,
    snarl: Snarl<IoNode>,
    built_for: Option<(u64, u64, Scope)>,
    positions: HashMap<u32, Pos2>,
    wire_notes: HashMap<(OutPinId, InPinId), String>,
}

impl Default for IoGraph {
    fn default() -> Self {
        IoGraph { scope: Scope::Selection, snarl: Snarl::new(), built_for: None, positions: HashMap::new(), wire_notes: HashMap::new() }
    }
}

/// `*` wildcard match, case-insensitive, as VBSP/the engine does for entity names.
fn glob(pat: &str, name: &str) -> bool {
    let (pat, name) = (pat.to_ascii_lowercase(), name.to_ascii_lowercase());
    if !pat.contains('*') {
        return pat == name;
    }
    let parts: Vec<&str> = pat.split('*').collect();
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !name.starts_with(first) || name.len() < first.len() + last.len() || !name.ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    true
}

fn note(c: &Connection) -> String {
    let mut s = Vec::new();
    if !c.param.is_empty() {
        s.push(format!("'{}'", c.param));
    }
    if c.delay > 0.0 {
        s.push(format!("{}s", crate::formats::vmf::fmt(c.delay)));
    }
    if c.times == 1 {
        s.push("once".into());
    }
    s.join(" ")
}

fn entity_title(e: &Entity) -> String {
    match e.get("targetname").filter(|n| !n.is_empty()) {
        Some(n) => format!("{n}\n{}", e.classname()),
        None => format!("{} #{}", e.classname(), e.id),
    }
}

fn collect_edges(map: &Map) -> Vec<Edge> {
    let mut edges = Vec::new();
    for e in &map.entities {
        for c in &e.connections {
            if c.target.is_empty() {
                continue;
            }
            // !activator, !self, !caller, !player ... have no static target
            if c.target.starts_with('!') {
                continue;
            }
            let hits: Vec<u32> = map.entities.iter().filter(|t| t.get("targetname").is_some_and(|n| glob(&c.target, n))).map(|t| t.id).collect();
            let n = note(c);
            if hits.is_empty() {
                edges.push(Edge { from: e.id, output: c.output.clone(), to: Key::Ghost(c.target.clone()), input: c.input.clone(), note: n.clone() });
            }
            for t in hits {
                edges.push(Edge { from: e.id, output: c.output.clone(), to: Key::Ent(t), input: c.input.clone(), note: n.clone() });
            }
        }
    }
    edges
}

/// Entity ids connected (in either direction) to any of `seeds`.
fn chain(edges: &[Edge], seeds: &BTreeSet<u32>) -> BTreeSet<Key> {
    let mut seen: BTreeSet<Key> = BTreeSet::new();
    let mut q: VecDeque<Key> = seeds.iter().map(|&i| Key::Ent(i)).collect();
    for k in &q {
        seen.insert(k.clone());
    }
    while let Some(k) = q.pop_front() {
        for e in edges {
            let (a, b) = (Key::Ent(e.from), e.to.clone());
            let next = if a == k {
                Some(b)
            } else if b == k {
                Some(a)
            } else {
                None
            };
            if let Some(n) = next {
                if seen.insert(n.clone()) {
                    q.push_back(n);
                }
            }
        }
    }
    seen
}

impl PartialOrd for Key {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Key {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        match (self, o) {
            (Key::Ent(a), Key::Ent(b)) => a.cmp(b),
            (Key::Ent(_), Key::Ghost(_)) => std::cmp::Ordering::Less,
            (Key::Ghost(_), Key::Ent(_)) => std::cmp::Ordering::Greater,
            (Key::Ghost(a), Key::Ghost(b)) => a.cmp(b),
        }
    }
}

/// Rough rendered height of a node: header plus one row per pin.
fn node_height(n: &IoNode) -> f32 {
    52.0 + 24.0 * n.inputs.len().max(n.outputs.len()).max(1) as f32
}

/// Tree-like layered layout. Columns are the longest path from the roots (cycles are cut at
/// their back edges), rows are ordered to reduce crossings, and every node sits next to the
/// middle of its parents.
fn layout(order: &[Key], edges: &[(Key, Key)], heights: &HashMap<Key, f32>) -> HashMap<Key, Pos2> {
    let n = order.len();
    let idx: HashMap<&Key, usize> = order.iter().enumerate().map(|(i, k)| (k, i)).collect();
    let mut succ: Vec<Vec<usize>> = vec![vec![]; n];
    let mut has_pred = vec![false; n];
    for (a, b) in edges {
        let (a, b) = (idx[a], idx[b]);
        if a != b && !succ[a].contains(&b) {
            succ[a].push(b);
            has_pred[b] = true;
        }
    }

    // DFS: preorder + drop back edges so the rest is a DAG
    let mut state = vec![0u8; n]; // 0 new, 1 on stack, 2 done
    let mut pre = vec![usize::MAX; n];
    let mut next_pre = 0;
    let mut fwd: Vec<Vec<usize>> = vec![vec![]; n];
    let roots: Vec<usize> = (0..n).filter(|&i| !has_pred[i]).chain(0..n).collect();
    for r in roots {
        if state[r] != 0 {
            continue;
        }
        state[r] = 1;
        pre[r] = next_pre;
        next_pre += 1;
        let mut stack = vec![(r, 0usize)];
        while let Some((u, i)) = stack.pop() {
            if i < succ[u].len() {
                stack.push((u, i + 1));
                let v = succ[u][i];
                match state[v] {
                    0 => {
                        fwd[u].push(v);
                        state[v] = 1;
                        pre[v] = next_pre;
                        next_pre += 1;
                        stack.push((v, 0));
                    }
                    1 => {} // back edge
                    _ => fwd[u].push(v),
                }
            } else {
                state[u] = 2;
            }
        }
    }
    let mut pred: Vec<Vec<usize>> = vec![vec![]; n];
    for u in 0..n {
        for &v in &fwd[u] {
            pred[v].push(u);
        }
    }

    // longest-path layers (Kahn)
    let mut layer = vec![0usize; n];
    let mut indeg: Vec<usize> = pred.iter().map(|p| p.len()).collect();
    let mut q: VecDeque<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    while let Some(u) = q.pop_front() {
        for &v in &fwd[u] {
            layer[v] = layer[v].max(layer[u] + 1);
            indeg[v] -= 1;
            if indeg[v] == 0 {
                q.push_back(v);
            }
        }
    }
    let nl = layer.iter().copied().max().map_or(0, |m| m + 1);
    let mut cols: Vec<Vec<usize>> = vec![vec![]; nl];
    for i in 0..n {
        cols[layer[i]].push(i);
    }
    for c in &mut cols {
        c.sort_by_key(|&i| pre[i]);
    }

    // barycenter sweeps to cut crossings
    let mut rank = vec![0usize; n];
    let set_rank = |cols: &Vec<Vec<usize>>, rank: &mut Vec<usize>| {
        for c in cols {
            for (r, &i) in c.iter().enumerate() {
                rank[i] = r;
            }
        }
    };
    set_rank(&cols, &mut rank);
    for pass in 0..6 {
        let down = pass % 2 == 0;
        let range: Vec<usize> = if down { (1..nl).collect() } else { (0..nl.saturating_sub(1)).rev().collect() };
        for l in range {
            let nb = if down { &pred } else { &fwd };
            let mut keyed: Vec<(f32, usize)> = cols[l]
                .iter()
                .map(|&i| {
                    let b = if nb[i].is_empty() { rank[i] as f32 } else { nb[i].iter().map(|&j| rank[j] as f32).sum::<f32>() / nb[i].len() as f32 };
                    (b, i)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            cols[l] = keyed.into_iter().map(|(_, i)| i).collect();
            for (r, &i) in cols[l].iter().enumerate() {
                rank[i] = r;
            }
        }
    }

    // y: stack column 0, then place each node at the middle of its neighbours, keeping order
    let h = |i: usize| heights[&order[i]];
    let mut y = vec![0f32; n];
    for pass in 0..6 {
        let down = pass % 2 == 0;
        let range: Vec<usize> = if pass == 0 { (0..nl).collect() } else if down { (1..nl).collect() } else { (0..nl.saturating_sub(1)).rev().collect() };
        for l in range {
            let nb = if down || pass == 0 { &pred } else { &fwd };
            let mut bottom = f32::NEG_INFINITY;
            for &i in &cols[l] {
                let want = if nb[i].is_empty() {
                    if pass == 0 { bottom.max(0.0) } else { y[i] }
                } else {
                    nb[i].iter().map(|&j| y[j] + h(j) / 2.0).sum::<f32>() / nb[i].len() as f32 - h(i) / 2.0
                };
                y[i] = want.max(bottom);
                bottom = y[i] + h(i) + GAP_Y;
            }
        }
    }
    let top = y.iter().copied().fold(f32::INFINITY, f32::min);
    let top = if top.is_finite() { top } else { 0.0 };
    order.iter().enumerate().map(|(i, k)| (k.clone(), Pos2::new(layer[i] as f32 * COL_W, y[i] - top))).collect()
}

fn push_unique(v: &mut Vec<String>, s: &str) -> usize {
    match v.iter().position(|x| x == s) {
        Some(i) => i,
        None => {
            v.push(s.to_string());
            v.len() - 1
        }
    }
}

impl IoGraph {
    fn rebuild(&mut self, map: &Map, sel: &BTreeSet<u32>) {
        let all = collect_edges(map);
        let edges: Vec<Edge> = match self.scope {
            Scope::Map => all,
            Scope::Selection => {
                let keep = chain(&all, sel);
                all.into_iter().filter(|e| keep.contains(&Key::Ent(e.from)) || keep.contains(&e.to)).collect()
            }
        };

        // node set in stable order: selected entities with no wires still show up alone
        let mut order: Vec<Key> = Vec::new();
        let add = |k: Key, order: &mut Vec<Key>| {
            if !order.contains(&k) {
                order.push(k);
            }
        };
        if self.scope == Scope::Selection {
            for &id in sel {
                add(Key::Ent(id), &mut order);
            }
        }
        for e in &edges {
            add(Key::Ent(e.from), &mut order);
            add(e.to.clone(), &mut order);
        }
        order.retain(|k| match k {
            Key::Ent(id) => map.entities.iter().any(|e| e.id == *id),
            Key::Ghost(_) => true,
        });

        // pins
        let mut nodes: HashMap<Key, IoNode> = HashMap::new();
        for k in &order {
            let title = match k {
                Key::Ent(id) => map.entities.iter().find(|e| e.id == *id).map(entity_title).unwrap_or_default(),
                Key::Ghost(n) => format!("{n}\n(missing entity)"),
            };
            nodes.insert(k.clone(), IoNode { key: k.clone(), title, outputs: vec![], inputs: vec![] });
        }
        let mut wires: Vec<(Key, usize, Key, usize, String)> = Vec::new();
        for e in &edges {
            let (from, to) = (Key::Ent(e.from), e.to.clone());
            if !nodes.contains_key(&from) || !nodes.contains_key(&to) {
                continue;
            }
            let o = push_unique(&mut nodes.get_mut(&from).unwrap().outputs, &e.output);
            let i = push_unique(&mut nodes.get_mut(&to).unwrap().inputs, &e.input);
            wires.push((from, o, to, i, e.note.clone()));
        }

        let heights: HashMap<Key, f32> = order.iter().map(|k| (k.clone(), node_height(&nodes[k]))).collect();
        let edge_keys: Vec<(Key, Key)> = wires.iter().map(|w| (w.0.clone(), w.2.clone())).collect();
        let auto_pos = layout(&order, &edge_keys, &heights);

        // snarl
        self.snarl = Snarl::new();
        self.wire_notes.clear();
        let mut ids: HashMap<Key, NodeId> = HashMap::new();
        for k in &order {
            let auto = auto_pos[k];
            let pos = match k {
                Key::Ent(id) => self.positions.get(id).copied().unwrap_or(auto),
                Key::Ghost(_) => auto,
            };
            let node = nodes.remove(k).unwrap();
            ids.insert(k.clone(), self.snarl.insert_node(pos, node));
        }
        for (from, o, to, i, n) in wires {
            let (out, inp) = (OutPinId { node: ids[&from], output: o }, InPinId { node: ids[&to], input: i });
            self.snarl.connect(out, inp);
            if !n.is_empty() {
                let e = self.wire_notes.entry((out, inp)).or_default();
                if !e.is_empty() {
                    e.push_str("; ");
                }
                e.push_str(&n);
            }
        }
    }

    /// Draw the graph. `version` is `Doc::version`, `sel_stamp` is `App::sel_stamp`.
    /// Returns an entity the user double-clicked.
    pub fn show(&mut self, ui: &mut egui::Ui, map: &Map, version: u64, sel: &BTreeSet<u32>, sel_stamp: u64) -> Option<u32> {
        let mut relayout = false;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.scope, Scope::Selection, "Selection chain");
            ui.selectable_value(&mut self.scope, Scope::Map, "Whole map");
            if ui.button("Re-layout").clicked() {
                relayout = true;
            }
            ui.label(RichText::new("double-click a header to select the entity").small().weak());
        });
        if relayout {
            self.positions.clear();
            self.built_for = None;
        }
        let stamp = if self.scope == Scope::Selection { sel_stamp } else { 0 };
        let want = Some((version, stamp, self.scope));
        if self.built_for != want {
            self.rebuild(map, sel);
            self.built_for = want;
        }
        if self.snarl.node_ids().next().is_none() {
            ui.label(match self.scope {
                Scope::Selection => "Select an entity that has outputs or is targeted by other entities.",
                Scope::Map => "No entity I/O connections in this map.",
            });
        }

        let mut picked = None;
        let mut viewer = Viewer { sel, picked: &mut picked, notes: &self.wire_notes };
        SnarlWidget::new().id_salt("io_graph").show(&mut self.snarl, &mut viewer, ui);

        for (pos, node) in self.snarl.nodes_pos() {
            if let Key::Ent(id) = node.key {
                self.positions.insert(id, pos);
            }
        }
        picked
    }
}

struct Viewer<'a> {
    sel: &'a BTreeSet<u32>,
    picked: &'a mut Option<u32>,
    notes: &'a HashMap<(OutPinId, InPinId), String>,
}

impl SnarlViewer<IoNode> for Viewer<'_> {
    fn title(&mut self, node: &IoNode) -> String {
        node.title.clone()
    }

    fn show_header(&mut self, node: NodeId, _inputs: &[InPin], _outputs: &[OutPin], ui: &mut egui::Ui, snarl: &mut Snarl<IoNode>) {
        let n = &snarl[node];
        let mut text = RichText::new(&n.title).strong();
        match n.key {
            Key::Ent(id) if self.sel.contains(&id) => text = text.color(Color32::from_rgb(255, 200, 80)),
            Key::Ghost(_) => text = text.color(Color32::from_rgb(220, 90, 90)),
            _ => {}
        }
        let r = ui.add(egui::Label::new(text).selectable(false).sense(egui::Sense::click()));
        if r.double_clicked() {
            if let Key::Ent(id) = n.key {
                *self.picked = Some(id);
            }
        }
    }

    fn inputs(&mut self, node: &IoNode) -> usize {
        node.inputs.len()
    }

    fn show_input(&mut self, pin: &InPin, ui: &mut egui::Ui, snarl: &mut Snarl<IoNode>) -> impl egui_snarl::ui::SnarlPin + 'static {
        ui.label(&snarl[pin.id.node].inputs[pin.id.input]);
        PinInfo::circle().with_fill(Color32::from_rgb(110, 170, 255))
    }

    fn outputs(&mut self, node: &IoNode) -> usize {
        node.outputs.len()
    }

    fn show_output(&mut self, pin: &OutPin, ui: &mut egui::Ui, snarl: &mut Snarl<IoNode>) -> impl egui_snarl::ui::SnarlPin + 'static {
        ui.label(&snarl[pin.id.node].outputs[pin.id.output]);
        PinInfo::triangle().with_fill(Color32::from_rgb(240, 150, 70))
    }

    fn has_wire_widget(&mut self, from: &OutPinId, to: &InPinId, _snarl: &Snarl<IoNode>) -> bool {
        self.notes.contains_key(&(*from, *to))
    }

    fn show_wire_widget(&mut self, from: &OutPin, to: &InPin, ui: &mut egui::Ui, _snarl: &mut Snarl<IoNode>) {
        if let Some(n) = self.notes.get(&(from.id, to.id)) {
            ui.label(RichText::new(n).small());
        }
    }

    // read-only graph: ignore edits
    fn connect(&mut self, _from: &OutPin, _to: &InPin, _snarl: &mut Snarl<IoNode>) {}
    fn disconnect(&mut self, _from: &OutPin, _to: &InPin, _snarl: &mut Snarl<IoNode>) {}
    fn drop_outputs(&mut self, _pin: &OutPin, _snarl: &mut Snarl<IoNode>) {}
    fn drop_inputs(&mut self, _pin: &InPin, _snarl: &mut Snarl<IoNode>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ent(id: u32, name: &str, conns: &[(&str, &str)]) -> Entity {
        let mut e = Entity { id, ..Default::default() };
        e.props.push(("classname".into(), "logic_relay".into()));
        if !name.is_empty() {
            e.props.push(("targetname".into(), name.into()));
        }
        e.connections = conns.iter().map(|(o, v)| Connection::parse(&(o.to_string(), v.to_string()))).collect();
        e
    }

    #[test]
    fn connection_roundtrip() {
        for v in ["door,Open,,0.5,1", "door\x1bOpen\x1b\x1b0.5\x1b1"] {
            let pair = ("OnTrigger".to_string(), v.to_string());
            let c = Connection::parse(&pair);
            assert_eq!((c.target.as_str(), c.input.as_str(), c.delay, c.times), ("door", "Open", 0.5, 1));
            assert_eq!(c.to_pair(), pair);
        }
    }

    #[test]
    fn layout_is_a_tree() {
        // a -> b, a -> c, b -> d, c -> d, d -> a (cycle must not hang)
        let order: Vec<Key> = (1..=4).map(Key::Ent).collect();
        let e = |a: u32, b: u32| (Key::Ent(a), Key::Ent(b));
        let edges = vec![e(1, 2), e(1, 3), e(2, 4), e(3, 4), e(4, 1)];
        let heights: HashMap<Key, f32> = order.iter().map(|k| (k.clone(), 80.0)).collect();
        let p = layout(&order, &edges, &heights);
        let (a, b, c, d) = (p[&Key::Ent(1)], p[&Key::Ent(2)], p[&Key::Ent(3)], p[&Key::Ent(4)]);
        assert!(a.x < b.x && b.x == c.x && b.x < d.x);
        assert!((b.y - c.y).abs() >= 80.0, "siblings overlap");
        assert!(a.y > b.y.min(c.y) - 1.0 && a.y < b.y.max(c.y) + 1.0, "parent sits between children");
    }

    #[test]
    fn wildcard() {
        assert!(glob("door*", "Door_1"));
        assert!(glob("*_1", "door_1"));
        assert!(glob("a*c*e", "abcde"));
        assert!(!glob("door*", "xdoor"));
        assert!(glob("door", "DOOR"));
    }

    #[test]
    fn chain_and_ghost() {
        let map = Map {
            entities: vec![
                ent(1, "btn", &[("OnPressed", "relay,Trigger,,0,-1")]),
                ent(2, "relay", &[("OnTrigger", "door*,Open,,1,1"), ("OnTrigger", "nothing,Kill,,0,-1"), ("OnTrigger", "!activator,Kill,,0,-1")]),
                ent(3, "door_a", &[]),
                ent(4, "lonely", &[]),
            ],
            ..Default::default()
        };
        let edges = collect_edges(&map);
        assert_eq!(edges.len(), 3);
        assert!(edges.iter().any(|e| e.to == Key::Ghost("nothing".into())));
        let c = chain(&edges, &[3].into_iter().collect());
        assert!(c.contains(&Key::Ent(1)) && c.contains(&Key::Ent(2)));
        assert!(!c.contains(&Key::Ent(4)));

        let mut g = IoGraph::default();
        g.scope = Scope::Map;
        g.rebuild(&map, &BTreeSet::new());
        assert_eq!(g.snarl.node_ids().count(), 4); // btn, relay, door_a, ghost
        assert_eq!(g.snarl.wires().count(), 3);
    }
}
