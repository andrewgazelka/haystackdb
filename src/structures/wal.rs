use crate::constants::{QUANTIZED_VECTOR_SIZE, VECTOR_SIZE};

use super::{
    metadata_index::KVPair,
    mmap_tree::{
        serialization::{TreeDeserialization, TreeSerialization},
        Tree,
    },
};
use crate::utils::quantize;
use std::hash::{Hash, Hasher};
use std::{
    fmt::Display,
    hash::DefaultHasher,
    io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub struct CommitListItem {
    pub hash: u64,
    pub timestamp: u64,
    pub vectors: Vec<[u8; QUANTIZED_VECTOR_SIZE]>,
    pub kvs: Vec<Vec<KVPair>>,
}

impl Display for CommitListItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CommitListItem {{ hash: {}, timestamp: {}}}",
            self.hash, self.timestamp
        )
    }
}

impl TreeSerialization for CommitListItem {
    fn write_to<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.hash.write_to(writer)?;
        self.timestamp.write_to(writer)?;
        self.vectors.write_to(writer)?;
        self.kvs.write_to(writer)?;
        Ok(())
    }

    fn serialized_size(&self) -> usize {
        self.hash.serialized_size()
            + self.timestamp.serialized_size()
            + self.vectors.serialized_size()
            + self.kvs.serialized_size()
    }
}

impl TreeDeserialization for CommitListItem {
    fn read_from<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let hash = u64::read_from(reader)?;
        let timestamp = u64::read_from(reader)?;
        let vectors = Vec::read_from(reader)?;
        let kvs = Vec::read_from(reader)?;

        Ok(CommitListItem {
            hash,
            timestamp,
            kvs,
            vectors,
        })
    }
}

impl TreeSerialization for bool {
    fn write_to<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&[*self as u8])
    }

    fn serialized_size(&self) -> usize {
        1
    }
}

impl TreeDeserialization for bool {
    fn read_from<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let byte = u8::read_from(reader)?;
        Ok(byte == 1)
    }
}

pub struct WAL {
    pub commit_list: Tree<u64, CommitListItem>,
    pub timestamps: Tree<u64, Vec<u64>>, // maps a timestamp to a hash
    pub commit_finish: Tree<u64, bool>,
    pub path: PathBuf,
    pub namespace_id: String,
}

impl WAL {
    pub fn new(path: PathBuf, namespace_id: String) -> io::Result<Self> {
        let commit_list_path = path.clone().join("commit_list.bin");
        let commit_list = Tree::<u64, CommitListItem>::new(commit_list_path)?;
        let timestamps_path = path.clone().join("timestamps.bin");
        let timestamps = Tree::<u64, Vec<u64>>::new(timestamps_path)?;
        let commit_finish_path = path.clone().join("commit_finish.bin");
        let commit_finish = Tree::<u64, bool>::new(commit_finish_path)?;

        Ok(WAL {
            commit_list,
            path,
            namespace_id,
            timestamps,
            commit_finish,
        })
    }

    pub fn add_to_commit_list(
        &mut self,
        hash: u64,
        vectors: Vec<[u8; QUANTIZED_VECTOR_SIZE]>,
        kvs: Vec<Vec<KVPair>>,
    ) -> Result<(), io::Error> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let commit_list_item = CommitListItem {
            hash,
            timestamp,
            vectors,
            kvs,
        };

        self.commit_list.insert(hash, commit_list_item)?;
        // Flush to ensure durability for WAL
        self.commit_list.flush()?;

        // self.commit_finish.insert(hash, false)?;

        // self.timestamps.insert(timestamp, hash)?;

        Ok(())
    }

    pub fn has_been_committed(&mut self, hash: u64) -> Result<bool, io::Error> {
        match self.commit_list.has_key(hash) {
            Ok(r) => Ok(r),
            Err(_) => Ok(false),
        }
    }

    pub fn get_commits(&mut self) -> Result<Vec<CommitListItem>, io::Error> {
        let start = 0;
        let end = u64::MAX;

        let commits = self
            .commit_list
            .get_range(start, end)
            .expect("Error getting commits");

        Ok(commits.into_iter().map(|(_, v)| v).collect())
    }

    pub fn get_commit(&mut self, hash: u64) -> Result<Option<CommitListItem>, io::Error> {
        match self.commit_list.search(hash) {
            Ok(v) => Ok(v),
            Err(_) => Ok(None),
        }
    }

    pub fn get_commits_before(&mut self, timestamp: u64) -> Result<Vec<CommitListItem>, io::Error> {
        let hash_end = self.timestamps.get_range(0, timestamp)?;

        let mut commits = Vec::new();

        for (_, hash) in hash_end {
            for h in hash {
                if let Ok(Some(c)) = self.commit_list.search(h) {
                    commits.push(c);
                }
            }
        }

        // println!("Commits before: {:?}", commits.len());

        Ok(commits)
    }

    pub fn get_uncommitted(&mut self, last_seconds: u64) -> Result<Vec<CommitListItem>, io::Error> {
        let start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - last_seconds;

        let end = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 1;

        let all_hashes = self.timestamps.get_range(start, end)?;

        let mut commits = Vec::new();

        for (_, hashes) in all_hashes {
            for hash in hashes {
                if let Ok(has_key) = self.commit_finish.has_key(hash) {
                    if !has_key {
                        if let Ok(Some(c)) = self.commit_list.search(hash) {
                            commits.push(c);
                        }
                    }
                }
            }
        }

        // commits.dedup_by_key(|c| c.hash);

        Ok(commits)
    }

    pub fn compute_hash(
        &self,
        vectors: &Vec<[u8; QUANTIZED_VECTOR_SIZE]>,
        kvs: &Vec<Vec<KVPair>>,
    ) -> u64 {
        let mut hasher = DefaultHasher::default();

        // for vector in vectors {
        //     vector.hash(&mut hasher);
        // }
        vectors.hash(&mut hasher);

        kvs.hash(&mut hasher);

        hasher.finish()
    }

    pub fn add_to_wal(
        &mut self,
        vectors: Vec<[f32; VECTOR_SIZE]>,
        kvs: Vec<Vec<KVPair>>,
    ) -> io::Result<()> {
        if vectors.len() != kvs.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Quantized vectors length mismatch",
            ));
        }

        let quantized_vectors: Vec<[u8; QUANTIZED_VECTOR_SIZE]> =
            vectors.iter().map(quantize).collect();

        let hash = self.compute_hash(&quantized_vectors, &kvs);

        let current_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // println!("Current timestamp: {}", current_timestamp);

        let mut current_timestamp_vals = match self.timestamps.search(current_timestamp) {
            Ok(v) => v,
            Err(_) => Some(Vec::new()),
        }
        .unwrap_or(Vec::new());

        current_timestamp_vals.push(hash);

        self.timestamps
            .insert(current_timestamp, current_timestamp_vals)?;
        self.timestamps.flush()?;

        self.add_to_commit_list(hash, quantized_vectors, kvs)?;

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

        let quantized_vectors: Vec<Vec<[u8; QUANTIZED_VECTOR_SIZE]>> = vectors
            .iter()
            .map(|v| v.iter().map(quantize).collect())
            .collect();

        let mut hashes = Vec::new();

        let current_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut current_timestamp_vals = match self.timestamps.search(current_timestamp) {
            Ok(v) => v,
            Err(_) => Some(Vec::new()),
        }
        .unwrap_or(Vec::new());

        for (v, k) in quantized_vectors.iter().zip(kvs.iter()) {
            let hash = self.compute_hash(v, k);
            hashes.push(hash);

            current_timestamp_vals.push(hash);
        }

        self.timestamps
            .insert(current_timestamp, current_timestamp_vals)?;
        self.timestamps.flush()?;

        for (hash, (v, k)) in hashes.iter().zip(quantized_vectors.iter().zip(kvs.iter())) {
            self.add_to_commit_list(*hash, v.clone(), k.clone())?;
        }

        Ok(())
    }

    pub fn mark_commit_finished(&mut self, hash: u64) -> io::Result<()> {
        self.commit_finish.insert(hash, true)?;
        self.commit_finish.flush()?;

        Ok(())
    }

    /// Flush all WAL components to disk for durability
    pub fn flush(&mut self) -> io::Result<()> {
        self.commit_list.flush()?;
        self.timestamps.flush()?;
        self.commit_finish.flush()?;
        Ok(())
    }
}
