use crate::domain::device_reset::{DeviceResetPhase, DeviceResetState, LocalCommandError};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Clone, Copy)]
struct LifecycleState {
    generation: u64,
    blocked: bool,
}
pub struct LocalLifecycle {
    state: RwLock<LifecycleState>,
    generation: AtomicU64,
}
pub struct OperationPermit<'a> {
    guard: RwLockReadGuard<'a, LifecycleState>,
}
pub struct ResetPermit<'a> {
    guard: RwLockWriteGuard<'a, LifecycleState>,
    generation: &'a AtomicU64,
}

impl LocalLifecycle {
    pub fn new(state: &DeviceResetState) -> Self {
        Self {
            state: RwLock::new(LifecycleState {
                generation: state.generation,
                blocked: matches!(
                    state.phase,
                    DeviceResetPhase::Pending | DeviceResetPhase::LocalCommitted
                ),
            }),
            generation: AtomicU64::new(state.generation),
        }
    }
    pub fn unavailable() -> Self {
        Self {
            state: RwLock::new(LifecycleState {
                generation: 0,
                blocked: true,
            }),
            generation: AtomicU64::new(0),
        }
    }
    pub async fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    pub async fn enter(&self, expected: u64) -> Result<OperationPermit<'_>, LocalCommandError> {
        let guard = self.state.read().await;
        validate_generation(*guard, expected)?;
        if guard.blocked {
            return Err(LocalCommandError::new(
                guard.generation,
                "reset_recovery_required",
                "기기 초기화 복구가 필요합니다",
            ));
        }
        Ok(OperationPermit { guard })
    }
    pub async fn enter_reset(&self, expected: u64) -> Result<ResetPermit<'_>, LocalCommandError> {
        let guard = self.state.write().await;
        validate_generation(*guard, expected)?;
        Ok(ResetPermit {
            guard,
            generation: &self.generation,
        })
    }
    pub async fn enter_recovery(&self) -> ResetPermit<'_> {
        ResetPermit {
            guard: self.state.write().await,
            generation: &self.generation,
        }
    }
}
fn validate_generation(state: LifecycleState, expected: u64) -> Result<(), LocalCommandError> {
    if state.generation != expected {
        Err(LocalCommandError::new(
            state.generation,
            "stale_generation",
            "기기 상태가 변경되어 다시 확인해야 합니다",
        ))
    } else {
        Ok(())
    }
}
impl OperationPermit<'_> {
    pub fn generation(&self) -> u64 {
        self.guard.generation
    }
}
impl ResetPermit<'_> {
    pub fn generation(&self) -> u64 {
        self.guard.generation
    }
    pub fn update(&mut self, state: &DeviceResetState) {
        self.guard.generation = state.generation;
        self.generation.store(state.generation, Ordering::SeqCst);
        self.guard.blocked = matches!(
            state.phase,
            DeviceResetPhase::Pending | DeviceResetPhase::LocalCommitted
        );
    }
    pub fn block(&mut self) {
        self.guard.blocked = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device_reset::DeviceResetPhase;
    #[test]
    fn local_reset_lifecycle_normal_work_allowed() {
        tauri::async_runtime::block_on(async {
            let lifecycle = LocalLifecycle::new(&DeviceResetState::default());
            assert_eq!(lifecycle.enter(0).await.unwrap().generation(), 0);
        });
    }
    #[test]
    fn local_reset_lifecycle_stale_generation_rejected() {
        tauri::async_runtime::block_on(async {
            let state = DeviceResetState {
                generation: 1,
                phase: DeviceResetPhase::Completed,
                ..Default::default()
            };
            let lifecycle = LocalLifecycle::new(&state);
            assert_eq!(
                lifecycle.enter(0).await.err().unwrap().code,
                "stale_generation"
            );
        });
    }
    #[test]
    fn local_reset_lifecycle_pending_blocks_normal_work() {
        tauri::async_runtime::block_on(async {
            let state = DeviceResetState {
                generation: 1,
                phase: DeviceResetPhase::Pending,
                ..Default::default()
            };
            let lifecycle = LocalLifecycle::new(&state);
            assert_eq!(
                lifecycle.enter(1).await.err().unwrap().code,
                "reset_recovery_required"
            );
        });
    }

    #[test]
    fn local_reset_lifecycle_waits_for_work_and_rejects_old_queued_generation() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        tauri::async_runtime::block_on(async {
            let lifecycle = LocalLifecycle::new(&DeviceResetState::default());
            let operation = lifecycle.enter(0).await.unwrap();
            let mut reset = std::pin::pin!(lifecycle.enter_reset(0));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(matches!(reset.as_mut().poll(&mut cx), Poll::Pending));
            let mut queued = std::pin::pin!(lifecycle.enter(0));
            assert!(matches!(queued.as_mut().poll(&mut cx), Poll::Pending));
            drop(operation);
            let mut reset = reset.await.unwrap();
            reset.update(&DeviceResetState {
                generation: 1,
                phase: DeviceResetPhase::Completed,
                ..Default::default()
            });
            drop(reset);
            assert_eq!(queued.await.err().unwrap().code, "stale_generation");
            assert_eq!(lifecycle.enter(1).await.unwrap().generation(), 1);
        });
    }

    #[test]
    fn local_reset_lifecycle_generation_query_available_during_reset() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        tauri::async_runtime::block_on(async {
            let lifecycle = LocalLifecycle::new(&DeviceResetState::default());
            let mut reset = lifecycle.enter_reset(0).await.unwrap();
            reset.update(&DeviceResetState {
                generation: 1,
                phase: DeviceResetPhase::Pending,
                ..Default::default()
            });
            let mut query = std::pin::pin!(lifecycle.generation());
            let mut cx = Context::from_waker(Waker::noop());
            assert!(matches!(query.as_mut().poll(&mut cx), Poll::Ready(1)));
        });
    }
}
