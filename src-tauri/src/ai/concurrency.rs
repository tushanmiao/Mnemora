//! 按 provider 共享的模型请求许可池。
//!
//! DeepNote 会并行生成多个 Chunk/章节。如果只限制它自己的 worker 数，不同 run
//! 仍然可以叠加并把交互式 Chat 饿死。这里用一套 provider 级总许可，再给后台请求
//! 一套更小的许可：后台必须同时取得两者，Chat 只取总许可，因此始终保留至少一个
//! 交互槽位。

use std::{collections::HashMap, sync::Arc};

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use super::error::ModelError;

const DEFAULT_PROVIDER_CONCURRENCY: usize = 4;
const DEFAULT_INTERACTIVE_RESERVE: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRequestClass {
    Interactive,
    Background,
}

#[derive(Clone)]
struct ProviderGates {
    total: Arc<Semaphore>,
    background: Arc<Semaphore>,
}

#[derive(Debug)]
pub struct ProviderRequestPermit {
    _background: Option<OwnedSemaphorePermit>,
    _total: OwnedSemaphorePermit,
}

pub struct ProviderConcurrencyPool {
    gates: Mutex<HashMap<String, ProviderGates>>,
    total_limit: usize,
    background_limit: usize,
}

impl Default for ProviderConcurrencyPool {
    fn default() -> Self {
        Self::new(DEFAULT_PROVIDER_CONCURRENCY, DEFAULT_INTERACTIVE_RESERVE)
    }
}

impl ProviderConcurrencyPool {
    pub fn new(total_limit: usize, interactive_reserve: usize) -> Self {
        let total_limit = total_limit.max(1);
        let interactive_reserve = interactive_reserve.min(total_limit.saturating_sub(1));
        Self {
            gates: Mutex::new(HashMap::new()),
            total_limit,
            background_limit: total_limit.saturating_sub(interactive_reserve).max(1),
        }
    }

    async fn provider_gates(&self, provider_id: &str) -> ProviderGates {
        let mut gates = self.gates.lock().await;
        gates
            .entry(provider_id.to_string())
            .or_insert_with(|| ProviderGates {
                total: Arc::new(Semaphore::new(self.total_limit)),
                background: Arc::new(Semaphore::new(self.background_limit)),
            })
            .clone()
    }

    pub async fn acquire(
        &self,
        provider_id: &str,
        class: ProviderRequestClass,
        cancellation: &CancellationToken,
    ) -> Result<ProviderRequestPermit, ModelError> {
        let gates = self.provider_gates(provider_id).await;
        // 后台先取自己的闸门，避免占住总许可后排队等待后台名额。
        let background = if class == ProviderRequestClass::Background {
            Some(tokio::select! {
                _ = cancellation.cancelled() => return Err(ModelError::cancelled()),
                permit = gates.background.clone().acquire_owned() => permit
                    .map_err(|_| ModelError::provider("后台模型请求许可池已关闭。"))?,
            })
        } else {
            None
        };
        let total = tokio::select! {
            _ = cancellation.cancelled() => return Err(ModelError::cancelled()),
            permit = gates.total.clone().acquire_owned() => permit
                .map_err(|_| ModelError::provider("模型请求许可池已关闭。"))?,
        };
        Ok(ProviderRequestPermit {
            _background: background,
            _total: total,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn background_requests_cannot_consume_the_interactive_reserve() {
        let pool = Arc::new(ProviderConcurrencyPool::new(2, 1));
        let cancellation = CancellationToken::new();
        let background = pool
            .acquire(
                "provider-1",
                ProviderRequestClass::Background,
                &cancellation,
            )
            .await
            .unwrap();

        let waiting_pool = pool.clone();
        let waiting_cancellation = cancellation.clone();
        let waiting = tokio::spawn(async move {
            waiting_pool
                .acquire(
                    "provider-1",
                    ProviderRequestClass::Background,
                    &waiting_cancellation,
                )
                .await
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());

        let interactive = tokio::time::timeout(
            Duration::from_millis(100),
            pool.acquire(
                "provider-1",
                ProviderRequestClass::Interactive,
                &cancellation,
            ),
        )
        .await
        .expect("交互请求应立即取得预留槽")
        .unwrap();
        drop(interactive);
        drop(background);
        assert!(tokio::time::timeout(Duration::from_millis(100), waiting)
            .await
            .unwrap()
            .unwrap()
            .is_ok());
    }

    #[tokio::test]
    async fn pools_are_independent_between_providers_and_waits_are_cancellable() {
        let pool = Arc::new(ProviderConcurrencyPool::new(1, 0));
        let cancellation = CancellationToken::new();
        let first = pool
            .acquire(
                "provider-1",
                ProviderRequestClass::Interactive,
                &cancellation,
            )
            .await
            .unwrap();
        let other_provider = pool
            .acquire(
                "provider-2",
                ProviderRequestClass::Interactive,
                &cancellation,
            )
            .await
            .unwrap();

        let waiting_pool = pool.clone();
        let waiting_cancellation = CancellationToken::new();
        let cancellation_handle = waiting_cancellation.clone();
        let waiting = tokio::spawn(async move {
            waiting_pool
                .acquire(
                    "provider-1",
                    ProviderRequestClass::Interactive,
                    &waiting_cancellation,
                )
                .await
        });
        cancellation_handle.cancel();
        assert_eq!(
            waiting.await.unwrap().unwrap_err().kind,
            crate::ai::error::ModelErrorKind::Cancelled
        );
        drop(other_provider);
        drop(first);
    }
}
