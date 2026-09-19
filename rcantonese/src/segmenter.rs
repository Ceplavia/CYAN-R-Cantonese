// Jyutping syllable segmenter — port of Segmenter.cpp.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use crate::db::ImeDatabase;
use crate::types::*;

const MAX_SYLLABLE_KEY_COUNT: usize = 6;

struct SplitEdge {
        syllable: Syllable,
        end_index: usize,
}

struct SplitNode {
        syllable: Syllable,
        previous_index: Option<usize>,
        length: usize,
        count: usize,
}

fn single_scheme(syllable: Syllable) -> Segmentation {
        vec![vec![syllable]]
}

fn split_edges(keys: &[VirtualInputKey], syllables: &HashMap<i64, Syllable>, prefixes: &HashSet<i64>) -> Vec<Vec<SplitEdge>> {
        let input_length = keys.len();
        let mut edges: Vec<Vec<SplitEdge>> = (0..input_length).map(|_| Vec::new()).collect();
        for start_index in 0..input_length {
                let mut code = 0i64;
                let end_limit = input_length.min(start_index + MAX_SYLLABLE_KEY_COUNT);
                for end_index in start_index..end_limit {
                        code = code * 100 + keys[end_index].code;
                        if !prefixes.contains(&code) {
                                break;
                        }
                        if let Some(syllable) = syllables.get(&code) {
                                edges[start_index].push(SplitEdge {
                                        syllable: syllable.clone(),
                                        end_index: end_index + 1,
                                });
                        }
                }
        }
        edges
}

fn scheme_at(node_index: usize, nodes: &[SplitNode]) -> Scheme {
        let mut syllables = Vec::with_capacity(nodes[node_index].count);
        let mut current = Some(node_index);
        while let Some(index) = current {
                let node = &nodes[index];
                syllables.push(node.syllable.clone());
                current = node.previous_index;
        }
        syllables.reverse();
        syllables
}

pub struct Segmenter {
        syllables: HashMap<i64, Syllable>,
        prefixes: HashSet<i64>,
}

impl Segmenter {
        pub fn new() -> Self {
                Self {
                        syllables: HashMap::new(),
                        prefixes: HashSet::new(),
                }
        }

        pub fn prepare(&mut self, database: &ImeDatabase) -> bool {
                let rows = database.query_syllables();
                if rows.is_empty() {
                        self.syllables.clear();
                        self.prefixes.clear();
                        return false;
                }

                let mut syllables = HashMap::with_capacity(rows.len());
                for row in &rows {
                        syllables.insert(row.alias_code, Syllable::new(row.alias_code, row.origin_code));
                }

                let mut prefixes = HashSet::with_capacity(syllables.len() * MAX_SYLLABLE_KEY_COUNT);
                for syllable in syllables.values() {
                        let mut code = 0i64;
                        for key in &syllable.alias {
                                code = code * 100 + key.code;
                                prefixes.insert(code);
                        }
                }

                self.syllables = syllables;
                self.prefixes = prefixes;
                true
        }

        pub fn is_prepared(&self) -> bool {
                !self.syllables.is_empty()
        }

        pub fn segment(&self, keys: &[VirtualInputKey]) -> Segmentation {
                match keys.len() {
                        0 => return Segmentation::new(),
                        1 => {
                                let key = keys[0];
                                if key == LETTER_A {
                                        return single_scheme(Syllable::new(20, 2020));
                                }
                                if key == LETTER_O {
                                        return single_scheme(Syllable::new(34, 34));
                                }
                                if key == LETTER_M {
                                        return single_scheme(Syllable::new(32, 32));
                                }
                                return Segmentation::new();
                        }
                        _ => {}
                }

                let syllable_keys = syllable_keys(keys);
                let input_length = syllable_keys.len();
                if input_length == 0 || !self.is_prepared() {
                        return Segmentation::new();
                }

                let edges = split_edges(&syllable_keys, &self.syllables, &self.prefixes);
                if edges.is_empty() || edges[0].is_empty() {
                        return Segmentation::new();
                }

                let mut nodes: Vec<SplitNode> = Vec::new();
                let mut frontier: Vec<usize> = Vec::new();
                for edge in &edges[0] {
                        nodes.push(SplitNode {
                                syllable: edge.syllable.clone(),
                                previous_index: None,
                                length: edge.end_index,
                                count: 1,
                        });
                        frontier.push(nodes.len() - 1);
                }

                while !frontier.is_empty() {
                        let mut next_frontier = Vec::new();
                        for node_index in frontier.drain(..) {
                                let node_length = nodes[node_index].length;
                                let node_count = nodes[node_index].count;
                                if node_length >= input_length {
                                        continue;
                                }
                                for edge in &edges[node_length] {
                                        nodes.push(SplitNode {
                                                syllable: edge.syllable.clone(),
                                                previous_index: Some(node_index),
                                                length: edge.end_index,
                                                count: node_count + 1,
                                        });
                                        next_frontier.push(nodes.len() - 1);
                                }
                        }
                        frontier = next_frontier;
                }

                struct CandidateScheme {
                        scheme: Scheme,
                        length: usize,
                }

                let mut candidates: Vec<CandidateScheme> = nodes
                        .iter()
                        .enumerate()
                        .map(|(index, node)| CandidateScheme {
                                scheme: scheme_at(index, &nodes),
                                length: node.length,
                        })
                        .collect();

                candidates.sort_by(|a, b| b.length.cmp(&a.length)); // stable sort by length desc

                candidates.into_iter().map(|c| c.scheme).collect()
        }

        pub fn syllable_text(&self, keys: &[VirtualInputKey]) -> Option<String> {
                if keys.len() > MAX_SYLLABLE_KEY_COUNT {
                        return None;
                }
                self.lookup(combined_code(keys)).map(|s| s.origin_text())
        }

        pub fn lookup(&self, code: i64) -> Option<&Syllable> {
                self.syllables.get(&code)
        }

        /// Number of known syllable aliases (for InputMemory heuristics).
        pub fn syllable_count(&self) -> usize {
                self.syllables.len()
        }
}
