use std::sync::Arc;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use indicatif_log_bridge::LogWrapper;
use log::info;
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};

pub struct TasksManager {
    pub multi: MultiProgress,
}

impl TasksManager {
    pub fn init(level: log::LevelFilter) -> Self {
        let multi = MultiProgress::new();
        let logger = env_logger::Builder::new()
            .format_target(false)
            .filter_level(level)
            .build();
        LogWrapper::new(multi.clone(), logger).try_init().unwrap();
        Self { multi }
    }

    pub fn create<T, E>(&self, prefix: &'static str) -> Tasks<T, E> {
        Tasks {
            bar: self
                .multi
                .add(ProgressBar::new(0))
                .with_style(tasks_style())
                .with_prefix(prefix),
            tasks: Default::default(),
            semaphore: None,
        }
    }

    pub fn create_sync<T, E>(&self, prefix: &'static str) -> SyncTasks<T, E> {
        SyncTasks {
            bar: self
                .multi
                .add(ProgressBar::new(0))
                .with_style(tasks_style())
                .with_prefix(prefix),
            results: vec![],
        }
    }
}

pub struct Tasks<T, E> {
    semaphore: Option<Arc<Semaphore>>,
    bar: indicatif::ProgressBar,
    tasks: Mutex<JoinSet<Result<T, E>>>,
}

impl<T, E> Tasks<T, E> {
    pub fn limit(mut self, limit: usize) -> Self {
        self.semaphore = Some(Arc::new(Semaphore::new(limit)));
        self
    }

    async fn acquire_permit(&self) -> Option<OwnedSemaphorePermit> {
        match &self.semaphore {
            Some(semaphore) => Some(semaphore.clone().acquire_owned().await.unwrap()),
            None => None,
        }
    }
}

impl<T: 'static, E: 'static> Tasks<T, E> {
    pub async fn spawn<F>(&self, callback: F)
    where
        F: Future<Output = Result<T, E>> + Send + 'static,
        T: Send,
        E: Send,
    {
        self.bar.inc_length(1);
        let permit = self.acquire_permit().await;

        let bar = self.bar.clone();
        self.tasks.lock().await.spawn(async move {
            let result = callback.await;
            bar.inc(1);
            drop(permit);
            result
        });
    }

    pub async fn join_all(self) -> Vec<Result<T, E>> {
        let tasks = self.tasks.into_inner();
        let results = tasks.join_all().await;

        self.bar.finish_with_message("completed");

        let total = results.len();
        let done = results.iter().filter(|r| r.is_ok()).count();
        if total != 0 {
            info!("{} completed", self.bar.prefix());
            info!("done: {done}");
            if done != total {
                info!("failed: {}", total - done);
            }
            info!("total: {total}");
        }

        results
    }
}

pub struct SyncTasks<T, E> {
    pub bar: indicatif::ProgressBar,
    pub results: Vec<Result<T, E>>,
}

impl<T, E> SyncTasks<T, E> {
    pub async fn spawn<F>(&mut self, callback: F)
    where
        F: Future<Output = Result<T, E>>,
    {
        self.bar.inc_length(1);
        self.results.push(callback.await);
        self.bar.inc(1);
    }

    pub fn join_all(self) -> Vec<Result<T, E>> {
        self.bar.finish_with_message("completed");

        let total = self.results.len();
        let done = self.results.iter().filter(|r| r.is_ok()).count();
        if total != 0 {
            info!("{} completed", self.bar.prefix());
            info!("done: {done}");
            if done != total {
                info!("failed: {}", total - done);
            }
            info!("total: {total}");
        }

        self.results
    }
}

fn tasks_style() -> ProgressStyle {
    ProgressStyle::with_template("{prefix:.bold.dim} {wide_bar:.cyan/blue} {pos:>3}/{len:3}")
        .unwrap()
        .progress_chars("#>-")
}
