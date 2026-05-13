use std::{collections::HashMap, path::PathBuf};
use futures::future::try_join_all;
use log::error;
use plyne::{FnEevent, Output};
use tempfile::{TempDir, tempdir};
use tokio_util::task::TaskTracker;

use crate::{ArchiveClient, Result, progress::ProgressSet};

pub type DownloadEvent = FnEevent<Vec<String>, Result<(TempDir, HashMap<String, PathBuf>)>>;

pub async fn download_task(
    mut files_pipeline: Output<DownloadEvent>,
    progress_set: ProgressSet,
    client: ArchiveClient,
) {
    let progress = progress_set.add("files");
    let tasks = TaskTracker::new();

    while let Some((urls, ret)) = files_pipeline.recv().await {
        progress.inc_length(urls.len() as u64);
        let progress = progress.clone();
        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let client = client.clone();
        tasks.spawn(async move {
            let downloads = urls
                .into_iter()
                .map(|url| {
                    let progress = progress.clone();
                    let client = client.clone();
                    let path = temp_dir.path().to_path_buf();
                    async move { 
                        let result = client.download(&path, url.clone()).await.map(|path| (url.clone(), path));
                        if result.is_err() {
                            error!("Failed to download file: {}", url);
                        }
                        progress.inc(1); //TODO: handle try_join_all failed download
                        result
                    }
                })
                .collect::<Vec<_>>();

            let result = try_join_all(downloads)
                .await
                .map(|results| (temp_dir, results.into_iter().collect::<HashMap<_, _>>()));

            ret.send(result).ok();
        });
    }

    tasks.wait().await;
}
