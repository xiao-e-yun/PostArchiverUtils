use std::sync::Arc;

use log::info;
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};

pub struct TaskSet<T, E> {
    name: &'static str,
    semaphore: Option<Arc<Semaphore>>,
    tasks: Mutex<JoinSet<Result<T, E>>>,
}

impl<T, E> TaskSet<T, E> {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            semaphore: None,
            tasks: Default::default(),
        }
    }

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

impl<T: 'static, E: 'static> TaskSet<T, E> {
    pub async fn spawn<F>(&self, callback: F)
    where
        F: Future<Output = Result<T, E>> + Send + 'static,
        T: Send,
        E: Send,
    {
        let permit = self.acquire_permit().await;

        self.tasks.lock().await.spawn(async move {
            let result = callback.await;
            drop(permit);
            result
        });
    }

    pub async fn join_all(self) -> Vec<Result<T, E>> {
        let tasks = self.tasks.into_inner();
        let results = tasks.join_all().await;

        let total = results.len();
        let done = results.iter().filter(|r| r.is_ok()).count();
        if total != 0 {
            info!("{} completed", self.name);
            info!("done: {done}");
            if done != total {
                info!("failed: {}", total - done);
            }
            info!("total: {total}");
        }

        results
    }
}
