pub mod display;
pub mod error;
pub mod request;
pub mod tasks;
pub mod params;

use std::path::Path;

use log::info;
use post_archiver::manager::PostArchiverManager;
pub use request::*;
pub use tasks::*;
pub use error::*;
pub use params::*;

pub fn setup_database(path: &Path) -> Result<PostArchiverManager> {
    std::fs::create_dir_all(path)?;
    info!("Connecting to PostArchiver");
    Ok(PostArchiverManager::open_or_create(path)?)
}
