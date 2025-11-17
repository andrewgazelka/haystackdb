use crate::constants::VECTOR_SIZE;
use crate::structures::inverted_index::InvertedIndexItem;
use crate::structures::metadata_index::{KVPair, MetadataIndexItem};

use super::namespace_state::NamespaceState;
use std::collections::HashMap;

use std::io;
use std::os::unix::fs as unix_fs;
use std::path::PathBuf;
use tracing::{info, debug};

pub struct CommitService {
    pub state: NamespaceState,
}

impl CommitService {
    pub fn new(path: PathBuf, namespace_id: String) -> io::Result<Self> {
        let state = NamespaceState::new(path, namespace_id)?;
        Ok(CommitService { state })
    }

    pub fn commit(&mut self) -> io::Result<()> {
        let commits = self.state.wal.get_uncommitted(100000)?;

        let commits_len = commits.len();

        if commits.is_empty() {
            return Ok(());
        }

        info!(commits_len, "Starting commit processing");

        let merged_commits = commits
            .iter()
            .fold((Vec::new(), Vec::new()), |mut items, commit| {
                let vectors = commit.vectors.clone();
                let kvs = commit.kvs.clone();

                items.0.extend(vectors);
                items.1.extend(kvs);

                items
            });

        for (processed, (vectors, kvs)) in [merged_commits].iter().enumerate() {
            if vectors.len() != kvs.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Quantized vectors length mismatch",
                ));
            }

            debug!(
                processed,
                total = commits_len,
                vectors_len = vectors.len(),
                "Processing commit"
            );

            // generate u128 ids

            let ids = (0..vectors.len())
                .map(|_| uuid::Uuid::new_v4().as_u128())
                .collect::<Vec<u128>>();

            debug!(ids_len = ids.len(), "Generated IDs");

            let vector_indices = self.state.vectors.batch_push(vectors.clone())?;

            debug!(indices_count = vector_indices.len(), "Pushed vectors");

            let mut inverted_index_items: HashMap<KVPair, Vec<(usize, u128)>> = HashMap::new();
            let mut batch_metadata_to_insert = Vec::new();

            for (idx, kv) in kvs.iter().enumerate() {
                let metadata_index_item = MetadataIndexItem {
                    id: ids[idx],
                    kvs: kv.clone(),
                    vector_index: vector_indices[idx],
                };

                batch_metadata_to_insert.push((ids[idx], metadata_index_item));

                for kv in kv {
                    inverted_index_items
                        .entry(kv.clone())
                        .or_default()
                        .push((vector_indices[idx], ids[idx]));
                }
            }

            self.state
                .metadata_index
                .batch_insert(batch_metadata_to_insert);

            for (kv, items) in inverted_index_items {
                let inverted_index_item = InvertedIndexItem {
                    indices: items.iter().map(|(idx, _)| *idx).collect(),
                    ids: items.iter().map(|(_, id)| *id).collect(),
                };

                self.state
                    .inverted_index
                    .insert_append(kv, inverted_index_item);
            }
        }

        for commit in commits {
            self.state.wal.mark_commit_finished(commit.hash)?;
        }

        Ok(())
    }

    pub fn recover_point_in_time(&mut self, timestamp: u64) -> io::Result<()> {
        info!(timestamp, "Starting point-in-time recovery");
        let versions: Vec<i32> = self.state.get_all_versions()?;
        let max_version = versions.iter().max().unwrap();
        let new_version = max_version + 1;

        debug!(?versions, "Available versions");

        info!(new_version, "Creating new version");

        let new_version_path = self
            .state
            .path
            .parent()
            .unwrap()
            .join(format!("v{}", new_version));

        let mut fresh_state =
            NamespaceState::new(new_version_path.clone(), self.state.namespace_id.clone())?;

        let commits = self.state.wal.get_commits_before(timestamp)?;
        let commits_len = commits.len();

        if commits.is_empty() {
            return Ok(());
        }

        info!(commits_len, "Processing commits for PITR");

        for (processed, commit) in commits.iter().enumerate() {
            let vectors = commit.vectors.clone();
            let kvs = commit.kvs.clone();

            if vectors.len() != kvs.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Quantized vectors length mismatch",
                ));
            }

            debug!(
                processed,
                total = commits_len,
                vectors_len = vectors.len(),
                "Processing PITR commit"
            );

            // generate u128 ids
            let ids = (0..vectors.len())
                .map(|_| uuid::Uuid::new_v4().as_u128())
                .collect::<Vec<u128>>();

            debug!(ids_len = ids.len(), "Generated IDs");

            let vector_indices = fresh_state.vectors.batch_push(vectors)?;

            debug!(indices_count = vector_indices.len(), "Pushed vectors");

            let mut inverted_index_items: HashMap<KVPair, Vec<(usize, u128)>> = HashMap::new();

            let mut metadata_index_items = Vec::new();

            for (idx, kv) in kvs.iter().enumerate() {
                let metadata_index_item = MetadataIndexItem {
                    id: ids[idx],
                    kvs: kv.clone(),
                    vector_index: vector_indices[idx],
                };

                metadata_index_items.push((ids[idx], metadata_index_item));

                for kv in kv {
                    inverted_index_items
                        .entry(kv.clone())
                        .or_default()
                        .push((vector_indices[idx], ids[idx]));
                }
            }

            fresh_state
                .metadata_index
                .batch_insert(metadata_index_items);

            for (kv, items) in inverted_index_items {
                let inverted_index_item = InvertedIndexItem {
                    indices: items.iter().map(|(idx, _)| *idx).collect(),
                    ids: items.iter().map(|(_, id)| *id).collect(),
                };

                fresh_state
                    .inverted_index
                    .insert_append(kv, inverted_index_item);
            }

            fresh_state.wal.mark_commit_finished(commit.hash)?;
        }

        // update symlink for /current
        let current_path = self.state.path.clone();

        info!(?current_path, "Updating current symlink");

        std::fs::remove_file(&current_path)?;
        unix_fs::symlink(&new_version_path, &current_path)?;

        Ok(())
    }

    pub fn add_to_wal(
        &mut self,
        vectors: Vec<[f32; VECTOR_SIZE]>,
        kvs: Vec<Vec<KVPair>>,
    ) -> io::Result<()> {
        if vectors.len() != vectors.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Quantized vectors length mismatch",
            ));
        }

        // self.state.wal.commit(hash, quantized_vectors, kvs)
        self.state
            .wal
            .add_to_wal(vectors, kvs)
            .expect("Failed to add to wal");

        Ok(())
    }

    pub fn batch_add_to_wal(
        &mut self,
        vectors: Vec<Vec<[f32; VECTOR_SIZE]>>,
        kvs: Vec<Vec<Vec<KVPair>>>,
    ) -> io::Result<()> {
        if vectors.len() != kvs.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Quantized vectors length mismatch",
            ));
        }

        self.state.wal.batch_add_to_wal(vectors, kvs)?;

        Ok(())
    }
}
