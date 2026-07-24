//! First-run initialization.
//!
//! New installations deliberately start in official-login mode for both Claude
//! applications. Third-party providers are only added explicitly by the user or
//! through the per-application import action.

use crate::error::AppResult;

pub fn run_seed(_conn: &rusqlite::Connection) -> AppResult<()> {
    Ok(())
}
