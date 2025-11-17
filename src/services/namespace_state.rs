use crate::structures::dense_vector_list::DenseVectorList;
use crate::structures::inverted_index::InvertedIndex;
use crate::structures::metadata_index::MetadataIndex;
use crate::structures::wal::WAL;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use super::LockService;

pub struct NamespaceState {
    pub namespace_id: String,
    pub metadata_index: MetadataIndex,
    pub inverted_index: InvertedIndex,
    pub vectors: DenseVectorList,
    pub wal: WAL,
    pub locks: LockService,
    pub path: PathBuf,
}

fn get_all_versions(path: &Path) -> io::Result<Vec<i32>> {
    let mut versions = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let version = path.file_name().unwrap().to_str().unwrap().to_string();
            // parse as int without `v` to see if it's a version
            // must remove the v first
            if let Some(stripped) = version.strip_prefix("v") {
                if let Ok(version) = stripped.parse::<i32>() {
                    versions.push(version);
                }
            }
        }
    }
    Ok(versions)
}

impl NamespaceState {
    pub fn new(path: PathBuf, namespace_id: String) -> io::Result<Self> {
        // path should be .../current, which should be a symlink to the current version

        if !path.exists() {
            fs::create_dir_all(path.clone().parent().unwrap())
                .expect("Failed to create directory");
        }

        let versions = get_all_versions(path.clone().parent().unwrap())?;

        if versions.is_empty() {
            // create v0
            let version_path = path.clone().parent().unwrap().join("v0");
            fs::create_dir_all(&version_path).expect("Failed to create directory");

            // create symlink

            std::os::unix::fs::symlink(&version_path, &path).expect("Failed to create symlink");
        }

        let metadata_path = path.clone().join("metadata.bin");
        let inverted_index_path = path.clone().join("inverted_index.bin");
        let wal_path = path.clone().join("wal");
        let locks_path = path.clone().join("locks");

        fs::create_dir_all(&wal_path).expect("Failed to create directory");

        fs::create_dir_all(&locks_path).expect("Failed to create directory");

        let vectors_path = path.clone().join("vectors.bin");

        let metadata_index = MetadataIndex::new(metadata_path);
        let inverted_index = InvertedIndex::new(inverted_index_path);
        let wal = WAL::new(wal_path, namespace_id.clone())?;
        let vectors = DenseVectorList::new(vectors_path, 100_000)?;
        let locks = LockService::new(locks_path);

        Ok(NamespaceState {
            namespace_id,
            metadata_index,
            inverted_index,
            vectors,
            wal,
            locks,
            path,
        })
    }

    pub fn get_all_versions(&self) -> io::Result<Vec<i32>> {
        let mut versions = Vec::new();
        for entry in fs::read_dir(self.path.parent().unwrap())? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let version = path.file_name().unwrap().to_str().unwrap().to_string();
                // parse as int without `v` to see if it's a version
                if let Some(stripped) = version.strip_prefix("v") {
                    if let Ok(version) = stripped.parse::<i32>() {
                        versions.push(version);
                    }
                }
            }
        }
        Ok(versions)
    }
}
