pub mod display;
pub mod error;
pub mod params;
pub mod request;
pub mod tasks;

use std::path::Path;

pub use error::*;
use log::info;
pub use params::*;
use post_archiver::manager::PostArchiverManager;
pub use request::*;
pub use tasks::*;

pub fn setup_database(path: &Path) -> Result<PostArchiverManager> {
    std::fs::create_dir_all(path)?;
    info!("Connecting to PostArchiver");
    Ok(PostArchiverManager::open_or_create(path)?)
}
