use std::fmt::Display;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::structures::mmap_tree::Tree;

use super::metadata_index::KVPair;
use super::mmap_tree::serialization::{TreeDeserialization, TreeSerialization};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InvertedIndexItem {
    pub indices: Vec<usize>,
    pub ids: Vec<u128>,
}

impl Display for InvertedIndexItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InvertedIndexItem {{ ... }}")
    }
}

impl TreeSerialization for InvertedIndexItem {
    fn write_to<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        (self.indices.len() as u64).write_to(writer)?;

        let len_of_index_bytes: usize = 8;
        len_of_index_bytes.write_to(writer)?;

        for index in &self.indices {
            index.write_to(writer)?;
        }

        (self.ids.len() as u64).write_to(writer)?;

        let len_of_id_bytes: usize = 16;
        len_of_id_bytes.write_to(writer)?;

        for id in &self.ids {
            id.write_to(writer)?;
        }

        Ok(())
    }

    fn serialized_size(&self) -> usize {
        8 + // indices.len()
        8 + // len_of_index_bytes
        self.indices.len() * 8 + // indices
        8 + // ids.len()
        8 + // len_of_id_bytes
        self.ids.len() * 16 // ids
    }
}

impl TreeDeserialization for InvertedIndexItem {
    fn read_from<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let indices_len = u64::read_from(reader)? as usize;
        let _len_of_index_bytes = usize::read_from(reader)?; // Always 8, ignored

        let mut indices = Vec::with_capacity(indices_len);
        for _ in 0..indices_len {
            indices.push(usize::read_from(reader)?);
        }

        let ids_len = u64::read_from(reader)? as usize;
        let _len_of_id_bytes = usize::read_from(reader)?; // Always 16, ignored

        let mut ids = Vec::with_capacity(ids_len);
        for _ in 0..ids_len {
            ids.push(u128::read_from(reader)?);
        }

        Ok(InvertedIndexItem { indices, ids })
    }
}

pub struct InvertedIndex {
    pub path: PathBuf,
    pub tree: Tree<KVPair, InvertedIndexItem>,
}

pub fn compress_indices(indices: Vec<usize>) -> Vec<usize> {
    let mut compressed = Vec::new();
    if indices.is_empty() {
        return compressed;
    }

    let mut current_start = indices[0];
    let mut count = 1;

    for item in indices.iter().skip(1) {
        if *item == current_start + count {
            count += 1;
        } else {
            compressed.push(current_start);
            compressed.push(count);
            current_start = *item;
            count = 1;
        }
    }
    compressed.push(current_start);
    compressed.push(count);

    compressed
}

pub fn decompress_indices(compressed: Vec<usize>) -> Vec<usize> {
    let mut decompressed = Vec::new();
    let mut i = 0;

    while i < compressed.len() {
        let start = compressed[i];
        let count = compressed[i + 1];
        decompressed.extend((start..start + count).collect::<Vec<usize>>());
        i += 2; // Move to the next pair
    }

    decompressed
}

impl InvertedIndex {
    pub fn new(path: PathBuf) -> Self {
        let tree = Tree::new(path.clone()).expect("Failed to create tree");
        InvertedIndex { path, tree }
    }

    pub fn insert(&mut self, key: KVPair, value: InvertedIndexItem, skip_compression: bool) {
        if !skip_compression {
            let compressed_indices = compress_indices(value.indices);
            let value = InvertedIndexItem {
                indices: compressed_indices,
                ids: value.ids,
            };
            self.tree.insert(key, value).expect("Failed to insert");
        } else {
            self.tree.insert(key, value).expect("Failed to insert");
        }
    }

    pub fn get(&mut self, key: KVPair) -> Option<InvertedIndexItem> {
        match self.tree.search(key) {
            Ok(v) => {
                match v {
                    Some(mut item) => {
                        item.indices = decompress_indices(item.indices);
                        Some(item)
                    }
                    None => None,
                }
            }
            Err(_) => None,
        }
    }

    pub fn insert_append(&mut self, key: KVPair, mut value: InvertedIndexItem) {
        match self.get(key.clone()) {
            Some(mut v) => {
                v.ids.extend(value.ids);

                let mut decompressed = v.indices.clone();

                // binary search to insert all of the ones to append
                for index in value.indices {
                    let idx = decompressed.binary_search(&index).unwrap_or_else(|x| x);
                    decompressed.insert(idx, index);
                }

                decompressed.sort_unstable();
                decompressed.dedup();

                v.indices = compress_indices(decompressed);

                self.insert(key, v, true);
            }
            None => {
                value.indices = compress_indices(value.indices);
                self.insert(key, value, true);
            }
        }
    }
}
