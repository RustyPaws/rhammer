//! FGD (Forge Game Data) parser: entity definitions, properties, choices, flags, I/O.

use glam::DVec3;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum ClassKind {
    Base,
    Point,
    Solid,
    Npc,
    Other,
}

#[derive(Clone, Debug, Default)]
pub struct Choice {
    pub value: String,
    pub label: String,
    /// For flags: default on?
    pub default_on: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Prop {
    pub name: String,
    pub ty: String,
    pub display: String,
    pub default: String,
    pub help: String,
    pub readonly: bool,
    pub choices: Vec<Choice>,
}

#[derive(Clone, Debug, Default)]
pub struct IoDef {
    pub name: String,
    pub ty: String,
    pub help: String,
}

#[derive(Clone, Debug)]
pub struct EntityClass {
    pub kind: ClassKind,
    pub name: String,
    pub desc: String,
    pub bases: Vec<String>,
    pub color: Option<[u8; 3]>,
    pub size: Option<(DVec3, DVec3)>,
    pub model: Option<String>,
    pub sprite: Option<String>,
    pub props: Vec<Prop>,
    pub inputs: Vec<IoDef>,
    pub outputs: Vec<IoDef>,
}

#[derive(Default)]
pub struct Fgd {
    pub classes: HashMap<String, EntityClass>,
    /// Sorted class names for UI listing.
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum T {
    Id(String),
    Str(String),
    Sym(char),
}

fn tokenize(src: &str) -> Vec<T> {
    let b: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && i + 1 < b.len() && b[i + 1] == '/' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '"' {
            i += 1;
            let st = i;
            while i < b.len() && b[i] != '"' {
                i += 1;
            }
            out.push(T::Str(b[st..i].iter().collect()));
            i += 1;
        } else if "()[]{}=:,+@".contains(c) {
            out.push(T::Sym(c));
            i += 1;
        } else {
            let st = i;
            while i < b.len() && !b[i].is_whitespace() && !"()[]{}=:,+@\"".contains(b[i]) {
                i += 1;
            }
            out.push(T::Id(b[st..i].iter().collect()));
        }
    }
    out
}

struct P<'a> {
    t: &'a [T],
    i: usize,
}

impl<'a> P<'a> {
    fn peek(&self) -> Option<&T> {
        self.t.get(self.i)
    }
    fn next(&mut self) -> Option<T> {
        let r = self.t.get(self.i).cloned();
        self.i += 1;
        r
    }
    fn sym(&mut self, c: char) -> bool {
        if self.peek() == Some(&T::Sym(c)) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    /// string literal with `+` concatenation
    fn string(&mut self) -> Option<String> {
        let mut s = match self.peek() {
            Some(T::Str(s)) => s.clone(),
            _ => return None,
        };
        self.i += 1;
        while self.peek() == Some(&T::Sym('+')) {
            self.i += 1;
            if let Some(T::Str(n)) = self.peek() {
                s.push_str(n);
                self.i += 1;
            }
        }
        Some(s)
    }
    /// raw tokens inside balanced parens (after '(' consumed)
    fn paren_args(&mut self) -> Vec<T> {
        let mut depth = 1;
        let mut out = Vec::new();
        while let Some(t) = self.next() {
            match t {
                T::Sym('(') => depth += 1,
                T::Sym(')') => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            out.push(t);
        }
        out
    }
}

fn nums(args: &[T]) -> Vec<f64> {
    args.iter()
        .filter_map(|t| match t {
            T::Id(s) | T::Str(s) => s.parse::<f64>().ok(),
            _ => None,
        })
        .collect()
}

fn strs(args: &[T]) -> Vec<String> {
    args.iter()
        .filter_map(|t| match t {
            T::Id(s) | T::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

impl Fgd {
    pub fn load(path: &Path) -> Fgd {
        let mut fgd = Fgd::default();
        let mut seen = Vec::new();
        fgd.load_file(path, &mut seen);
        fgd.resolve();
        fgd
    }

    fn load_file(&mut self, path: &Path, seen: &mut Vec<PathBuf>) {
        let canon = path.to_path_buf();
        if seen.contains(&canon) {
            return;
        }
        seen.push(canon);
        let Ok(bytes) = std::fs::read(path) else { return };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let toks = tokenize(&text);
        let mut p = P { t: &toks, i: 0 };
        while let Some(t) = p.next() {
            if t != T::Sym('@') {
                continue;
            }
            let Some(T::Id(kw)) = p.next() else { continue };
            let kw = kw.to_ascii_lowercase();
            match kw.as_str() {
                "include" => {
                    if let Some(f) = p.string() {
                        let inc = path.parent().unwrap_or(Path::new(".")).join(f);
                        self.load_file(&inc, seen);
                    }
                }
                "baseclass" | "pointclass" | "solidclass" | "npcclass" | "keyframeclass" | "moveclass"
                | "filterclass" => {
                    let kind = match kw.as_str() {
                        "baseclass" => ClassKind::Base,
                        "pointclass" => ClassKind::Point,
                        "solidclass" => ClassKind::Solid,
                        "npcclass" => ClassKind::Npc,
                        _ => ClassKind::Other,
                    };
                    if let Some(c) = parse_class(&mut p, kind) {
                        // BaseClass names live in their own namespace (e.g. BaseClass "Light" vs PointClass "light").
                        let key = if c.kind == ClassKind::Base {
                            format!("@base:{}", c.name.to_ascii_lowercase())
                        } else {
                            c.name.to_ascii_lowercase()
                        };
                        self.classes.insert(key, c);
                    }
                }
                _ => {
                    // @mapsize, @AutoVisGroup, ... skip to next '@' at top level
                    let mut depth = 0;
                    while let Some(t) = p.peek() {
                        match t {
                            T::Sym('[') => depth += 1,
                            T::Sym(']') => depth -= 1,
                            T::Sym('@') if depth <= 0 => break,
                            _ => {}
                        }
                        p.i += 1;
                    }
                }
            }
        }
    }

    fn resolve(&mut self) {
        // Flatten inheritance: base-class props come first, derived override by name.
        let names: Vec<String> = self.classes.keys().cloned().collect();
        let mut done: HashMap<String, EntityClass> = HashMap::new();
        for n in &names {
            let c = self.flatten(n, &mut Vec::new());
            if let Some(c) = c {
                done.insert(n.clone(), c);
            }
        }
        let mut vis: Vec<String> = done
            .values()
            .filter(|c| c.kind != ClassKind::Base)
            .map(|c| c.name.clone())
            .collect();
        vis.sort_by_key(|s| s.to_ascii_lowercase());
        self.classes = done;
        self.names = vis;
    }

    fn flatten(&self, name: &str, stack: &mut Vec<String>) -> Option<EntityClass> {
        let mut key = name.to_ascii_lowercase();
        if let Some(base_key) = Some(format!("@base:{key}")).filter(|k| !stack.contains(k) && self.classes.contains_key(k)) {
            // `base(X)` references prefer the BaseClass namespace; direct lookups (from resolve) pass the exact key.
            if !name.starts_with('@') && stack.last().is_some() {
                key = base_key;
            }
        }
        if stack.contains(&key) {
            return None;
        }
        let c = self.classes.get(&key)?.clone();
        stack.push(key);
        let mut out = c.clone();
        out.props.clear();
        out.inputs.clear();
        out.outputs.clear();
        let mut props: Vec<Prop> = Vec::new();
        let mut inputs: Vec<IoDef> = Vec::new();
        let mut outputs: Vec<IoDef> = Vec::new();
        for b in &c.bases {
            if let Some(bc) = self.flatten(b, stack) {
                merge(&mut props, bc.props, |p| p.name.clone());
                merge(&mut inputs, bc.inputs, |p| p.name.clone());
                merge(&mut outputs, bc.outputs, |p| p.name.clone());
                if out.color.is_none() {
                    out.color = bc.color;
                }
                if out.size.is_none() {
                    out.size = bc.size;
                }
                if out.model.is_none() {
                    out.model = bc.model;
                }
                if out.sprite.is_none() {
                    out.sprite = bc.sprite;
                }
            }
        }
        merge(&mut props, c.props, |p| p.name.clone());
        merge(&mut inputs, c.inputs, |p| p.name.clone());
        merge(&mut outputs, c.outputs, |p| p.name.clone());
        out.props = props;
        out.inputs = inputs;
        out.outputs = outputs;
        stack.pop();
        Some(out)
    }

    pub fn get(&self, class: &str) -> Option<&EntityClass> {
        self.classes.get(&class.to_ascii_lowercase())
    }

    pub fn point_classes(&self) -> impl Iterator<Item = &EntityClass> {
        self.names.iter().filter_map(|n| self.get(n)).filter(|c| c.kind == ClassKind::Point || c.kind == ClassKind::Npc)
    }

    pub fn solid_classes(&self) -> impl Iterator<Item = &EntityClass> {
        self.names.iter().filter_map(|n| self.get(n)).filter(|c| c.kind == ClassKind::Solid)
    }
}

fn merge<T: Clone>(dst: &mut Vec<T>, src: Vec<T>, key: impl Fn(&T) -> String) {
    for s in src {
        let k = key(&s).to_ascii_lowercase();
        if let Some(pos) = dst.iter().position(|d| key(d).to_ascii_lowercase() == k) {
            dst[pos] = s;
        } else {
            dst.push(s);
        }
    }
}

fn parse_class(p: &mut P, kind: ClassKind) -> Option<EntityClass> {
    let mut c = EntityClass {
        kind,
        name: String::new(),
        desc: String::new(),
        bases: vec![],
        color: None,
        size: None,
        model: None,
        sprite: None,
        props: vec![],
        inputs: vec![],
        outputs: vec![],
    };
    // helpers until '='
    loop {
        match p.next()? {
            T::Sym('=') => break,
            T::Id(h) => {
                if p.sym('(') {
                    let args = p.paren_args();
                    match h.to_ascii_lowercase().as_str() {
                        "base" => c.bases = strs(&args),
                        "color" => {
                            let n = nums(&args);
                            if n.len() >= 3 {
                                c.color = Some([n[0] as u8, n[1] as u8, n[2] as u8]);
                            }
                        }
                        "size" => {
                            let n = nums(&args);
                            if n.len() >= 6 {
                                c.size = Some((DVec3::new(n[0], n[1], n[2]), DVec3::new(n[3], n[4], n[5])));
                            } else if n.len() >= 3 {
                                let h = DVec3::new(n[0], n[1], n[2]) / 2.0;
                                c.size = Some((-h, h));
                            }
                        }
                        "studio" | "studioprop" => c.model = strs(&args).into_iter().next(),
                        "iconsprite" | "sprite" => c.sprite = strs(&args).into_iter().next(),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(T::Id(n)) = p.next() {
        c.name = n;
    } else {
        return None;
    }
    if p.sym(':') {
        c.desc = p.string().unwrap_or_default();
    }
    if !p.sym('[') {
        return Some(c);
    }
    // body
    loop {
        match p.peek()? {
            T::Sym(']') => {
                p.i += 1;
                break;
            }
            T::Id(id) => {
                let id = id.clone();
                p.i += 1;
                let lower = id.to_ascii_lowercase();
                if lower == "input" || lower == "output" {
                    let Some(T::Id(name)) = p.next() else { continue };
                    let mut io = IoDef { name, ..Default::default() };
                    if p.sym('(') {
                        io.ty = strs(&p.paren_args()).join("");
                    }
                    if p.sym(':') {
                        if let Some(s) = p.string() {
                            io.help = s;
                        }
                    }
                    if lower == "input" {
                        c.inputs.push(io)
                    } else {
                        c.outputs.push(io)
                    }
                    continue;
                }
                // property: name(type) [readonly] : "display" : default : "help" [= [choices]]
                let mut prop = Prop { name: id, ..Default::default() };
                if p.sym('(') {
                    prop.ty = strs(&p.paren_args()).join("").to_ascii_lowercase();
                }
                if let Some(T::Id(r)) = p.peek() {
                    if r.eq_ignore_ascii_case("readonly") {
                        prop.readonly = true;
                        p.i += 1;
                    }
                }
                if p.sym(':') {
                    prop.display = p.string().unwrap_or_default();
                    if p.sym(':') {
                        // default (may be empty, string or number)
                        match p.peek() {
                            Some(T::Str(_)) => prop.default = p.string().unwrap_or_default(),
                            Some(T::Id(v)) => {
                                prop.default = v.clone();
                                p.i += 1;
                            }
                            _ => {}
                        }
                        if p.sym(':') {
                            prop.help = p.string().unwrap_or_default();
                        }
                    }
                }
                if p.sym('=') && p.sym('[') {
                    let is_flags = prop.ty == "flags";
                    loop {
                        match p.peek() {
                            Some(T::Sym(']')) => {
                                p.i += 1;
                                break;
                            }
                            None => break,
                            _ => {}
                        }
                        let value = match p.next() {
                            Some(T::Id(v)) | Some(T::Str(v)) => v,
                            _ => continue,
                        };
                        let mut ch = Choice { value, ..Default::default() };
                        if p.sym(':') {
                            ch.label = p.string().unwrap_or_default();
                            if p.sym(':') {
                                if let Some(T::Id(d)) = p.peek() {
                                    ch.default_on = d == "1";
                                    p.i += 1;
                                }
                                // optional help after another ':'
                                if p.sym(':') {
                                    let _ = p.string();
                                }
                            }
                        }
                        prop.choices.push(ch);
                    }
                    if is_flags {
                        // default value = sum of default-on flags
                        let sum: i64 = prop.choices.iter().filter(|c| c.default_on).filter_map(|c| c.value.parse::<i64>().ok()).sum();
                        prop.default = sum.to_string();
                    }
                }
                c.props.push(prop);
            }
            _ => {
                p.i += 1;
            }
        }
    }
    Some(c)
}
