pub mod node;
pub mod serialization;
pub mod storage;

use std::fmt::{Debug, Display};
use std::io;
use std::path::PathBuf;
use tracing::{error, warn};

use node::{Node, NodeType};
use serialization::{TreeDeserialization, TreeSerialization};
use storage::StorageManager;

pub struct Tree<K, V> {
    pub branching_factor: usize,
    pub storage_manager: storage::StorageManager<K, V>,
}

impl<K, V> Tree<K, V>
where
    K: Clone + Ord + TreeSerialization + TreeDeserialization + Debug + Display,
    V: Clone + TreeSerialization + TreeDeserialization,
{
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let mut storage_manager = StorageManager::<K, V>::new(path)?;

        if storage_manager.used_space() == 0 {
            let mut root = Node::new_leaf(0);
            root.is_root = true;
            let root_offset: usize = storage_manager.store_node(&mut root)?;
            storage_manager.set_root_offset(root_offset);
        }

        Ok(Tree {
            storage_manager,
            branching_factor: 32,
        })
    }

    pub fn insert(&mut self, key: K, value: V) -> Result<(), io::Error> {
        let mut root = self
            .storage_manager
            .load_node(self.storage_manager.root_offset())?;

        if root.is_full() {
            let mut new_root = Node::new_internal(0);
            new_root.is_root = true;
            let (median, mut sibling) = root.split(self.branching_factor)?;
            root.is_root = false;
            self.storage_manager.store_node(&mut root)?;
            let sibling_offset = self.storage_manager.store_node(&mut sibling)?;
            new_root.keys.push(median);
            new_root.children.push(self.storage_manager.root_offset()); // old root offset
            new_root.children.push(sibling_offset); // new sibling offset
            new_root.is_root = true;
            self.storage_manager.store_node(&mut new_root)?;
            self.storage_manager.set_root_offset(new_root.offset);

            root.parent_offset = Some(new_root.offset);
            sibling.parent_offset = Some(new_root.offset);
            self.storage_manager.store_node(&mut root)?;
            self.storage_manager.store_node(&mut sibling)?;
        }
        self.insert_non_full(self.storage_manager.root_offset(), key, value, 0)?;

        Ok(())
    }

    fn insert_non_full(
        &mut self,
        node_offset: usize,
        key: K,
        value: V,
        depth: usize,
    ) -> Result<(), io::Error> {
        if depth > 100 {
            // Set a reasonable limit based on your observations
            error!(depth, "Recursion depth limit reached");
            return Ok(());
        }

        let mut node = self.storage_manager.load_node(node_offset)?;

        if node.node_type == NodeType::Leaf {
            let idx = node.keys.binary_search(&key).unwrap_or_else(|x| x);

            if node.keys.get(idx) == Some(&key) {
                node.values[idx] = Some(value);

                self.storage_manager.store_node(&mut node)?;
                if node.is_root {
                    self.storage_manager.set_root_offset(node.offset);
                }
            } else {
                node.keys.insert(idx, key);
                node.values.insert(idx, Some(value));

                self.storage_manager.store_node(&mut node)?;
                if node.is_root {
                    self.storage_manager.set_root_offset(node.offset);
                }
            }
        } else {
            let idx = node.keys.binary_search(&key).unwrap_or_else(|x| x); // Find the child to go to
            let child_offset = node.children[idx];
            let mut child = self.storage_manager.load_node(child_offset)?;

            if child.is_full() {
                let (median, mut sibling) = child.split(self.branching_factor)?;
                let sibling_offset = self.storage_manager.store_node(&mut sibling)?;

                node.keys.insert(idx, median.clone());
                node.children.insert(idx + 1, sibling_offset);
                self.storage_manager.store_node(&mut node)?;

                if key < median {
                    self.insert_non_full(child_offset, key, value, depth + 1)?;
                } else {
                    self.insert_non_full(sibling_offset, key, value, depth + 1)?;
                }
            } else {
                self.insert_non_full(child_offset, key, value, depth + 1)?;
            }
        }

        Ok(())
    }

    pub fn search(&mut self, key: K) -> Result<Option<V>, io::Error> {
        self.search_node(self.storage_manager.root_offset(), key)
    }

    fn search_node(&mut self, node_offset: usize, key: K) -> Result<Option<V>, io::Error> {
        let node = self.storage_manager.load_node(node_offset)?;

        match node.node_type {
            NodeType::Internal => {
                let idx = node.keys.binary_search(&key).unwrap_or_else(|x| x); // Find the child to go to
                self.search_node(node.children[idx], key)
            }
            NodeType::Leaf => match node.keys.binary_search(&key) {
                Ok(idx) => Ok(node.values[idx].clone()),
                Err(_) => Ok(None),
            },
        }
    }

    pub fn has_key(&mut self, key: K) -> Result<bool, io::Error> {
        self.has_key_node(self.storage_manager.root_offset(), key)
    }

    pub fn has_key_node(&mut self, node_offset: usize, key: K) -> Result<bool, io::Error> {
        let node = self.storage_manager.load_node(node_offset)?;

        match node.node_type {
            NodeType::Internal => {
                let idx = node.keys.binary_search(&key).unwrap_or_else(|x| x); // Find the child to go to
                self.has_key_node(node.children[idx], key)
            }
            NodeType::Leaf => Ok(node.keys.binary_search(&key).into_iter().next().is_some()),
        }
    }

    pub fn get_range(&mut self, start: K, end: K) -> Result<Vec<(K, V)>, io::Error> {
        let mut result = Vec::new();
        self.get_range_node(self.storage_manager.root_offset(), start, end, &mut result)?;
        Ok(result)
    }

    fn get_range_node(
        &mut self,
        node_offset: usize,
        start: K,
        end: K,
        result: &mut Vec<(K, V)>,
    ) -> Result<(), io::Error> {
        let node = self.storage_manager.load_node(node_offset)?;

        match node.node_type {
            NodeType::Internal => {
                let mut idx = node
                    .keys
                    .binary_search(&start.clone())
                    .unwrap_or_else(|x| x);
                if idx == node.keys.len() {
                    idx -= 1;
                }

                self.get_range_node(node.children[idx], start.clone(), end.clone(), result)?;

                while idx < node.keys.len() && node.keys[idx] < end {
                    self.get_range_node(
                        node.children[idx + 1],
                        start.clone(),
                        end.clone(),
                        result,
                    )?;
                    idx += 1;
                }
            }
            NodeType::Leaf => {
                let mut idx = node.keys.binary_search(&start).unwrap_or_else(|x| x);
                if node.keys.is_empty() {
                    return Ok(());
                }
                if idx == node.keys.len() {
                    idx -= 1;
                }

                while idx < node.keys.len() && node.keys[idx] < end {
                    if node.keys[idx] >= start {
                        result.push((node.keys[idx].clone(), node.values[idx].clone().unwrap()));
                    }
                    idx += 1;
                }
            }
        }

        Ok(())
    }

    pub fn batch_insert(&mut self, entries: Vec<(K, V)>) -> Result<(), io::Error> {
        if entries.is_empty() {
            warn!("No entries to insert in batch_insert");
            return Ok(());
        }

        let mut entries = entries;
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let entrypoint = self.find_entrypoint(entries[0].0.clone())?;

        let mut current_offset = entrypoint;
        let mut node = self.storage_manager.load_node(current_offset)?;

        for (key, value) in entries.iter() {
            while node.node_type == NodeType::Internal {
                // We should only be operating on leaf nodes in this loop
                let idx = node.keys.binary_search(key).unwrap_or_else(|x| x);
                current_offset = node.children[idx];
                node = self.storage_manager.load_node(current_offset)?;
            }

            if node.is_full() {
                let (median, mut sibling) = node.split(self.branching_factor)?;
                let sibling_offset = self.storage_manager.store_node(&mut sibling)?;
                self.storage_manager.store_node(&mut node)?; // Store changes to the original node after splitting

                if node.is_root {
                    // Create a new root if the current node is the root
                    let mut new_root = Node::new_internal(0);
                    new_root.is_root = true;
                    new_root.keys.push(median.clone());
                    new_root.children.push(current_offset); // old root offset
                    new_root.children.push(sibling_offset); // new sibling offset
                    new_root.parent_offset = None;
                    let new_root_offset = self.storage_manager.store_node(&mut new_root)?;
                    self.storage_manager.set_root_offset(new_root_offset);
                    node.is_root = false;
                    node.parent_offset = Some(new_root_offset);
                    sibling.parent_offset = Some(new_root_offset);
                    self.storage_manager.store_node(&mut node)?;
                    self.storage_manager.store_node(&mut sibling)?;
                } else {
                    // Update the parent node with the new median
                    let parent_offset = node.parent_offset.unwrap();
                    let mut parent = self.storage_manager.load_node(parent_offset)?;
                    let idx = parent
                        .keys
                        .binary_search(&median.clone())
                        .unwrap_or_else(|x| x);
                    parent.keys.insert(idx, median.clone());
                    parent.children.insert(idx + 1, sibling_offset);
                    self.storage_manager.store_node(&mut parent)?;
                }

                // Decide which node to continue insertion
                if *key >= median {
                    current_offset = sibling_offset;
                    node = sibling;
                }
            }

            // Insert the key into the correct leaf node
            let position = node.keys.binary_search(key).unwrap_or_else(|x| x);
            node.keys.insert(position, key.clone());
            node.values.insert(position, Some(value.clone()));
            self.storage_manager.store_node(&mut node)?; // Store changes after each insertion
        }

        Ok(())
    }

    fn find_entrypoint(&mut self, key: K) -> Result<usize, io::Error> {
        let mut current_offset = self.storage_manager.root_offset();
        let mut node = self.storage_manager.load_node(current_offset)?;

        while node.node_type == NodeType::Internal {
            let idx = node.keys.binary_search(&key).unwrap_or_else(|x| x);
            current_offset = node.children[idx];
            node = self.storage_manager.load_node(current_offset)?;
        }

        Ok(current_offset)
    }

    /// Flush all changes to disk for durability
    pub fn flush(&mut self) -> io::Result<()> {
        self.storage_manager.flush()
    }
}
