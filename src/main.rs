use haystackdb::constants::VECTOR_SIZE;
use haystackdb::services::CommitService;
use haystackdb::services::QueryService;
use haystackdb::structures::filters::Filter as QueryFilter;
use haystackdb::structures::metadata_index::KVPair;
use parking_lot::Mutex;
use std::sync::Arc;
use std::{self, path::PathBuf};
use tokio::time::{interval, Duration};
use tracing::info;

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use std::collections::HashMap;

type AppState = Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let active_namespaces = Arc::new(Mutex::new(HashMap::new()));

    let app = Router::new()
        .route("/query/:namespace_id", post(query_handler))
        .route("/addVector/:namespace_id", post(add_vector_handler))
        .route("/pitr/:namespace_id/:timestamp", get(pitr_handler))
        .with_state(active_namespaces);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    info!("Server listening on 0.0.0.0:8080");
    axum::serve(listener, app).await.unwrap();
}

async fn query_handler(
    Path(namespace_id): Path<String>,
    State(active_namespaces): State<AppState>,
    Json(body): Json<(Vec<f64>, QueryFilter, usize)>,
) -> Json<serde_json::Value> {
    let base_path = PathBuf::from(format!("/workspace/data/{}/current", namespace_id));
    ensure_namespace_initialized(&namespace_id, &active_namespaces, base_path.clone()).await;

    let mut query_service = QueryService::new(base_path, namespace_id.clone()).unwrap();
    let fvec = &body.0;
    let metadata = &body.1;
    let top_k = body.2;

    let mut vec: [f32; VECTOR_SIZE] = [0.0; VECTOR_SIZE];
    fvec.iter()
        .enumerate()
        .for_each(|(i, &val)| vec[i] = val as f32);

    let start = std::time::Instant::now();

    let search_result = query_service
        .query(&vec, metadata, top_k)
        .expect("Failed to query");

    let duration = start.elapsed();

    info!(?duration, "Query completed");
    Json(serde_json::to_value(search_result).unwrap())
}

async fn add_vector_handler(
    Path(namespace_id): Path<String>,
    State(active_namespaces): State<AppState>,
    Json(body): Json<(Vec<f64>, Vec<KVPair>, String)>,
) -> Json<&'static str> {
    let base_path = PathBuf::from(format!("/workspace/data/{}/current", namespace_id));

    ensure_namespace_initialized(&namespace_id, &active_namespaces, base_path.clone()).await;

    let mut commit_service = CommitService::new(base_path, namespace_id.clone()).unwrap();
    let fvec = &body.0;
    let metadata = &body.1;

    let mut vec: [f32; VECTOR_SIZE] = [0.0; VECTOR_SIZE];
    fvec.iter()
        .enumerate()
        .for_each(|(i, &val)| vec[i] = val as f32);

    commit_service
        .add_to_wal(vec![vec], vec![metadata.clone()])
        .expect("Failed to add to WAL");

    Json("Success")
}

async fn pitr_handler(
    Path((namespace_id, timestamp)): Path<(String, String)>,
    State(active_namespaces): State<AppState>,
) -> Json<&'static str> {
    info!(namespace_id = %namespace_id, "Executing PITR");
    let base_path = PathBuf::from(format!("/workspace/data/{}/current", namespace_id));

    ensure_namespace_initialized(&namespace_id, &active_namespaces, base_path.clone()).await;

    let mut commit_service = CommitService::new(base_path, namespace_id.clone()).unwrap();

    let timestamp = timestamp.parse::<u64>().unwrap();
    commit_service
        .recover_point_in_time(timestamp)
        .expect("Failed to PITR");

    Json("Success")
}

async fn ensure_namespace_initialized(
    namespace_id: &String,
    active_namespaces: &Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    base_path_for_async: PathBuf,
) {
    let mut namespaces = active_namespaces.lock();
    if !namespaces.contains_key(namespace_id) {
        let namespace_id_cloned = namespace_id.clone();
        let handle = tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(10));
            loop {
                interval.tick().await;
                info!(namespace_id = %namespace_id_cloned, "Starting commit");
                let start = std::time::Instant::now();
                let commit_worker = Arc::new(Mutex::new(
                    CommitService::new(base_path_for_async.clone(), namespace_id_cloned.clone())
                        .unwrap(),
                ));

                commit_worker.lock().commit().expect("Failed to commit");
                let duration = start.elapsed();
                info!("Commit worker took {:?} to complete", duration);
            }
        });
        namespaces.insert(namespace_id.clone(), handle);
    }
}
