use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

use super::classes::{DIRECT, class_of, is_allowed};

pub struct Tree {
    root: String,
    edges: BTreeMap<String, BTreeSet<String>>,
}

impl Tree {
    pub fn new(root: &str, edges: &[(&str, &str)]) -> Self {
        let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (from, to) in edges {
            map.entry((*from).to_owned())
                .or_default()
                .insert((*to).to_owned());
        }
        Self {
            root: root.to_owned(),
            edges: map,
        }
    }

    pub fn from_metadata(metadata: &Value, manifest_path: &str) -> Self {
        let packages = metadata["packages"].as_array().unwrap();
        let names: BTreeMap<&str, &str> = packages
            .iter()
            .map(|package| {
                (
                    package["id"].as_str().unwrap(),
                    package["name"].as_str().unwrap(),
                )
            })
            .collect();
        let root = packages
            .iter()
            .find(|package| package["manifest_path"].as_str() == Some(manifest_path))
            .map(|package| package["name"].as_str().unwrap())
            .unwrap();
        let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for node in metadata["resolve"]["nodes"].as_array().unwrap() {
            let from = names[node["id"].as_str().unwrap()];
            for dep in node["deps"].as_array().unwrap() {
                let normal = dep["dep_kinds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|kind| kind["kind"].is_null());
                if normal {
                    edges
                        .entry(from.to_owned())
                        .or_default()
                        .insert(names[dep["pkg"].as_str().unwrap()].to_owned());
                }
            }
        }
        Self {
            root: root.to_owned(),
            edges,
        }
    }

    pub fn reach(&self) -> BTreeMap<String, Vec<String>> {
        let mut paths = BTreeMap::from([(self.root.clone(), vec![self.root.clone()])]);
        let mut queue = VecDeque::from([self.root.clone()]);
        while let Some(name) = queue.pop_front() {
            let path = paths[&name].clone();
            for next in self.edges.get(&name).into_iter().flatten() {
                if !paths.contains_key(next) {
                    let mut longer = path.clone();
                    longer.push(next.clone());
                    paths.insert(next.clone(), longer);
                    queue.push_back(next.clone());
                }
            }
        }
        paths
    }

    pub fn direct(&self) -> BTreeSet<String> {
        self.edges.get(&self.root).cloned().unwrap_or_default()
    }

    pub fn network_violations(&self) -> Vec<String> {
        self.reach()
            .into_iter()
            .filter(|(name, _)| *name != self.root && !is_allowed(name))
            .filter_map(|(name, path)| {
                class_of(&name).map(|class| format!("{name} ({class}) via {}", path.join(" > ")))
            })
            .collect()
    }

    pub fn direct_violations(&self) -> Vec<String> {
        let allowed: BTreeSet<String> = DIRECT.iter().map(|name| (*name).to_owned()).collect();
        let direct = self.direct();
        let added = direct
            .difference(&allowed)
            .map(|name| format!("added {name}"));
        let removed = allowed
            .difference(&direct)
            .map(|name| format!("removed {name}"));
        added.chain(removed).collect()
    }
}
