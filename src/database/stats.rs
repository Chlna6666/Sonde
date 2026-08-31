use super::{
    dimension_rollup as dimension_rollup_repo,
    first_seen as first_seen_repo,
    telemetry_count as telemetry_count_repo,
    trends as trend_repo,
    user_rollup as user_rollup_repo,
    version_dimension as version_dimension_repo,
};

#[path = "stats_impl.rs"]
mod implementation;

pub use implementation::*;
