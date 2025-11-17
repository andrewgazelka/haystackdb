use std::fmt::Display;
use std::hash::Hash;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::structures::mmap_tree::Tree;

use super::mmap_tree::serialization::{TreeDeserialization, TreeSerialization};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KVPair {
    pub key: String,
    pub value: String,
}

impl KVPair {
    pub fn new(key: String, value: String) -> Self {
        KVPair { key, value }
    }
}

impl PartialEq for KVPair {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.value == other.value
    }
}

impl Eq for KVPair {}

impl Hash for KVPair {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.value.hash(state);
    }
}

impl PartialOrd for KVPair {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for KVPair {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key
            .cmp(&other.key)
            .then_with(|| self.value.cmp(&other.value))
    }
}

impl Display for KVPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KVPair {{ key: {}, value: {} }}", self.key, self.value)
    }
}

impl TreeSerialization for KVPair {
    fn write_to<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        (self.key.len() as u64).write_to(writer)?;
        writer.write_all(self.key.as_bytes())?;
        (self.value.len() as u64).write_to(writer)?;
        writer.write_all(self.value.as_bytes())
    }

    fn serialized_size(&self) -> usize {
        8 + self.key.len() + 8 + self.value.len()
    }
}

impl TreeDeserialization for KVPair {
    fn read_from<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let key = String::read_from(reader)?;
        let value = String::read_from(reader)?;
        Ok(KVPair { key, value })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MetadataIndexItem {
    pub kvs: Vec<KVPair>,
    pub id: u128,
    pub vector_index: usize,
    // pub namespaced_id: String,
}

impl Display for MetadataIndexItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MetadataIndexItem {{ kvs: {:?}, id: {}, vector_index: {}, namespaced_id:  }}",
            self.kvs, self.id, self.vector_index
        )
    }
}

impl TreeSerialization for MetadataIndexItem {
    fn write_to<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        (self.kvs.len() as u64).write_to(writer)?;

        for kv in &self.kvs {
            (kv.serialized_size() as u64).write_to(writer)?;
            kv.write_to(writer)?;
        }

        self.id.write_to(writer)?;
        self.vector_index.write_to(writer)
    }

    fn serialized_size(&self) -> usize {
        let mut size = 8; // kvs.len()
        for kv in &self.kvs {
            size += 8; // kv size prefix
            size += kv.serialized_size();
        }
        size += 16; // id (u128)
        size += 8; // vector_index (usize)
        size
    }
}

impl TreeDeserialization for MetadataIndexItem {
    fn read_from<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let kvs_len = u64::read_from(reader)? as usize;

        let mut kvs = Vec::new();
        for _ in 0..kvs_len {
            let _kv_len = u64::read_from(reader)?; // Size prefix (ignored, we read until complete)
            let kv = KVPair::read_from(reader)?;
            kvs.push(kv);
        }

        let id = u128::read_from(reader)?;
        let vector_index = usize::read_from(reader)?;

        Ok(MetadataIndexItem {
            kvs,
            id,
            vector_index,
        })
    }
}

pub struct MetadataIndex {
    pub path: PathBuf,
    pub tree: Tree<u128, MetadataIndexItem>,
}

impl MetadataIndex {
    pub fn new(path: PathBuf) -> Self {
        let tree = Tree::new(path.clone()).expect("Failed to create tree");
        MetadataIndex { path, tree }
    }

    pub fn insert(&mut self, key: u128, value: MetadataIndexItem) {
        // self.tree.insert(key, value).expect("Failed to insert");
        self.tree.insert(key, value).expect("Failed to insert");
    }

    pub fn batch_insert(&mut self, items: Vec<(u128, MetadataIndexItem)>) {
        self.tree
            .batch_insert(items)
            .expect("Failed to batch insert");
    }

    pub fn get(&mut self, key: u128) -> Option<MetadataIndexItem> {
        self.tree.search(key).unwrap_or_default()
    }
}
