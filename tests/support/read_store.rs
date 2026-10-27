//! Shared contract-test fixtures. Event and read models use separate schemas.
//! Tests arrange sequential commands and one ordered projection writer.
#![allow(dead_code)]

use crate::database;
use mini_cqrs_es::{
    CqrsError, EventConsumers, EventLog, EventStore, Projector, ReadModels, ReadModelsBuilder,
    toasty,
};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};
static URLS: LazyLock<Mutex<HashMap<PathBuf, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

use super::common_game::{GameAggregate, GameProjection, GameReadModel};
use super::common_hotel::{HotelAggregate, HotelProjection, HotelReadModel, RoomReadModel};

pub struct Databases {
    pub directory: tempfile::TempDir,
    pub events: EventLog,
    pub reads: toasty::Db,
}

impl Databases {
    pub async fn temporary() -> Result<Self, CqrsError> {
        let directory = tempfile::tempdir().map_err(read_error)?;
        let path = directory.path().join("events.db");
        let url = database::fresh_url(&path, false).await;
        if database::postgres() {
            std::fs::write(&path, []).map_err(read_error)?;
        }
        let events = EventLog::connect(&url).await.map_err(read_error)?;
        events.create_schema().await?;
        let reads = create(&directory.path().join("read_models.db")).await?;
        Ok(Self {
            directory,
            events,
            reads,
        })
    }
}

pub fn read_error(error: impl std::error::Error + Send + Sync + 'static) -> CqrsError {
    CqrsError::storage("read database operation", error)
}

pub fn read_models() -> ReadModelsBuilder {
    ReadModels::builder()
        .register::<HotelReadModel>()
        .register::<RoomReadModel>()
        .register::<GameReadModel>()
}

pub async fn open(path: &Path) -> Result<toasty::Db, CqrsError> {
    let url = URLS
        .lock()
        .unwrap()
        .get(path)
        .cloned()
        .expect("test target must be created first");
    Ok(read_models()
        .connect(&url)
        .await
        .map_err(read_error)?
        .database())
}

pub async fn create(path: &Path) -> Result<toasty::Db, CqrsError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(read_error)?;
    let url = database::fresh_url(path, true).await;
    let reads = read_models().connect(&url).await.map_err(read_error)?;
    reads.create_schema().await?;
    URLS.lock().unwrap().insert(path.to_owned(), url);
    Ok(reads.database())
}

pub fn projections(database: &toasty::Db) -> EventConsumers {
    EventConsumers::new()
        .with(Projector::new(database.clone(), HotelProjection))
        .with(Projector::new(database.clone(), GameProjection))
}

/// Call only with writers paused. Returns an unpublished replacement on success.
/// Failure leaves the incomplete destination for inspection; use a fresh file to retry.
/// Independent streams have no cross-stream ordering requirement in these examples.
pub async fn rebuild(
    events: &impl EventStore,
    destination: &Path,
) -> Result<toasty::Db, CqrsError> {
    let prepared = mini_cqrs_es::ReplayPlan::new()
        .with_aggregate::<HotelAggregate>()
        .with_aggregate::<GameAggregate>()
        .prepare(events)
        .await
        .map_err(|error| CqrsError::Other(error.into()))?;
    let database = create(destination).await?;
    prepared
        .run(&projections(&database))
        .await
        .map_err(|error| CqrsError::Other(error.into()))?;
    Ok(database)
}
