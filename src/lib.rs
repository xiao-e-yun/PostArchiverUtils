pub mod display;
pub mod request;
pub mod error;

use std::path::{Path, PathBuf};
use post_archiver::{PostId, POSTS_PRE_CHUNK};

pub use display::*;
pub use request::*;
pub use error::*;

pub fn get_post_path(root: impl AsRef<Path>, post: PostId) -> PathBuf {
    let mut path = root.as_ref().to_path_buf();
    path.push((*post / POSTS_PRE_CHUNK).to_string());
    path.push((*post % POSTS_PRE_CHUNK).to_string());
    path
}
