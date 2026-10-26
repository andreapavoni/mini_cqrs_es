//! Application read models, isolated from event history.

use crate::{Projection, ProjectionCheckpoint, Projector};
use toasty::schema::Model;

/// A read database shared by application queries and transactional projectors.
/// Library checkpoint storage is registered automatically. Register only your
/// own read models; event and snapshot tables belong in the event database.
#[derive(Clone)]
pub struct ReadModels {
    database: toasty::Db,
}

impl ReadModels {
    pub fn builder() -> ReadModelsBuilder {
        ReadModelsBuilder::default()
    }

    /// Handle for native Toasty model queries. Cloning shares this read pool.
    pub fn database(&self) -> toasty::Db {
        self.database.clone()
    }

    /// Creates the registered read tables and library checkpoints in a fresh,
    /// provisioned database. Existing schemas require application-owned upgrades.
    pub async fn create_schema(&self) -> Result<(), crate::CqrsError> {
        self.database
            .push_schema()
            .await
            .map_err(|error| crate::CqrsError::storage("create read schema", error))
    }

    /// Binds application projection logic to this database. Transactions,
    /// duplicate handling and checkpoint advancement are managed by the runner.
    pub fn projector<P: Projection>(&self, projection: P) -> Projector<P> {
        Projector::new(self.database(), projection)
    }
}

/// Register application models before opening a database. Each terminal method
/// consumes the builder because a connected database's model set is fixed.
pub struct ReadModelsBuilder {
    models: toasty::ModelSet,
}

impl Default for ReadModelsBuilder {
    fn default() -> Self {
        let mut models = toasty::ModelSet::new();
        ProjectionCheckpoint::register(&mut models);
        Self { models }
    }
}

impl ReadModelsBuilder {
    pub fn register<M: Model>(mut self) -> Self {
        M::register(&mut self.models);
        self
    }

    /// Connects to a provisioned database without creating or changing tables.
    pub async fn connect(self, url: &str) -> toasty::Result<ReadModels> {
        let database = toasty::Db::builder()
            .models(self.models)
            .connect(url)
            .await?;
        Ok(ReadModels { database })
    }
}
