use post_archiver::manager::PostArchiverManager;
use tokio::sync::Mutex;

pub mod download;
pub mod sync;

pub type Manager = Mutex<PostArchiverManager>;
