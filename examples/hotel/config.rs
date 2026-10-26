use mini_cqrs_es::anyhow::{Result, bail};

pub struct Config {
    pub events_url: String,
    pub reads_url: String,
}
impl Config {
    pub fn from_env() -> Result<Self> {
        let pair = (
            std::env::var("EVENTS_DATABASE_URL"),
            std::env::var("READ_MODELS_DATABASE_URL"),
        );
        let (events_url, reads_url) = match pair {
            (Ok(events), Ok(reads)) => (events, reads),
            (Err(std::env::VarError::NotPresent), Err(std::env::VarError::NotPresent))
                if cfg!(feature = "turso") =>
            {
                (
                    "turso:hotel-events.db".into(),
                    "turso:hotel-read-models.db".into(),
                )
            }
            _ => bail!(
                "set EVENTS_DATABASE_URL and READ_MODELS_DATABASE_URL; enable the matching driver feature (turso or postgresql)"
            ),
        };
        if events_url == reads_url {
            bail!("event and read-model databases must be separate");
        }
        Ok(Self {
            events_url,
            reads_url,
        })
    }
}
