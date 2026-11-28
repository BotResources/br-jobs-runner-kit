#[cfg(test)]
mod tests;

use serde_json::{Value, json};

use crate::chart_line::ChartLine;
use crate::env::Env;
use crate::error::Res;
use crate::guards::{PoolGuard, ValueGuard};

pub const POOL_IMAGE_FIELD: &str = "image";

pub fn image_sets(line: ChartLine, values_key: &str, image: &str) -> Vec<(String, Value)> {
    let key = match line {
        ChartLine::PoolResource => POOL_IMAGE_FIELD,
        ChartLine::Values => values_key,
    };
    vec![(key.to_owned(), json!(image))]
}

pub struct ImageChange {
    _values: Option<ValueGuard>,
    _pool: Option<PoolGuard>,
}

impl ImageChange {
    pub async fn set(env: &Env, image: &str) -> Res<Self> {
        let line = ChartLine::of(&env.chart_version)?;
        let sets = image_sets(line, &env.image_key, image);
        let mut change = Self {
            _values: None,
            _pool: None,
        };
        match line {
            ChartLine::PoolResource => {
                let mut guard = PoolGuard::new(env);
                guard.set(&sets).await?;
                change._pool = Some(guard);
            }
            ChartLine::Values => {
                let mut guard = ValueGuard::new(env);
                guard.set(&sets).await?;
                change._values = Some(guard);
            }
        }
        Ok(change)
    }
}
