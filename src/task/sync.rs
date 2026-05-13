use plyne::{FnEevent, Output};
use post_archiver::{AuthorId, PostId, error::Result, importer::{UnsyncAuthor, UnsyncPost}, manager::PostArchiverManager};
use tokio::sync::Mutex;

use crate::{progress::ProgressSet, task::Manager};

pub type SyncPostEvent = FnEevent<UnsyncPost, Result<PostId>>;

pub async fn sync_post_task(
    mut sync_pipeline: Output<SyncPostEvent>, 
    manager: &Manager,
    progress: &ProgressSet,
) {
    let progress = progress.add("posts");

    while let Some((post, ret)) = sync_pipeline.recv().await {
        let mut manager = manager.lock().await;
        let tx = manager.transaction().expect("Failed to start transaction");
        let result = tx.import_post(post, true);
        tx.commit().expect("Failed to commit transaction");
        ret.send(result.map(|(id, _, _)| id)).ok();
        progress.inc(1);
    }
}

pub type SyncAuthorEvent = FnEevent<UnsyncAuthor, Result<AuthorId>>;
pub async fn sync_author_task(mut sync_pipeline: Output<SyncAuthorEvent>, manager: &Mutex<PostArchiverManager>) {
    while let Some((author, ret)) = sync_pipeline.recv().await {
        let mut manager = manager.lock().await;
        let tx = manager.transaction().expect("Failed to start transaction");
        let result = tx.import_author(author);
        tx.commit().expect("Failed to commit transaction");
        ret.send(result.into()).ok();
    }
}
