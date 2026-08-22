//! Canonical remote builder capability model and scheduler.
//!
//! The scheduler selects among host-owned bindings. Callers then use the same
//! authenticated binding and transport used by the build executor.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};

use super::{ActionKey, BuildCapability, BuildResourcePool, RemoteBuildBinding};

/// Maximum capability and placement request for one action.
///
/// This is a data model only. It never contains an endpoint or credential;
/// those come from a host-owned [`RemoteBuildBinding`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBuildRequest {
    pub action: ActionKey,
    pub capabilities: BTreeSet<BuildCapability>,
    pub features: BTreeSet<String>,
    pub resource_pools: BTreeSet<BuildResourcePool>,
    pub platform: Option<String>,
    pub trust_domain: Option<String>,
    pub cache_read: bool,
    pub cache_write: bool,
    pub execute: bool,
    pub fallback_local: bool,
}

impl RemoteBuildRequest {
    pub fn new(action: ActionKey) -> Self {
        Self {
            action,
            capabilities: BTreeSet::new(),
            features: BTreeSet::new(),
            resource_pools: BTreeSet::new(),
            platform: None,
            trust_domain: None,
            cache_read: false,
            cache_write: false,
            execute: false,
            fallback_local: false,
        }
    }

    pub fn with_capability(mut self, capability: BuildCapability) -> Self {
        self.capabilities.insert(capability);
        self
    }

    pub fn with_feature(mut self, feature: impl Into<String>) -> Self {
        self.features.insert(feature.into());
        self
    }

    pub fn with_pool(mut self, pool: BuildResourcePool) -> Self {
        self.resource_pools.insert(pool);
        self
    }

    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }

    pub fn with_trust_domain(mut self, trust_domain: impl Into<String>) -> Self {
        self.trust_domain = Some(trust_domain.into());
        self
    }

    pub fn with_cache_read(mut self, enabled: bool) -> Self {
        self.cache_read = enabled;
        self
    }

    pub fn with_cache_write(mut self, enabled: bool) -> Self {
        self.cache_write = enabled;
        self
    }

    pub fn with_execute(mut self, enabled: bool) -> Self {
        self.execute = enabled;
        self
    }

    pub fn with_local_fallback(mut self, enabled: bool) -> Self {
        self.fallback_local = enabled;
        self
    }
}

/// Facts a host has granted to one remote worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBuilderCapabilities {
    pub platform: String,
    pub features: BTreeSet<String>,
    pub resource_pools: BTreeSet<BuildResourcePool>,
    pub capabilities: BTreeSet<BuildCapability>,
    pub concurrency: usize,
    pub priority: i32,
    pub trust_domain: String,
    pub cache_read: bool,
    pub cache_write: bool,
    pub execute: bool,
}

impl RemoteBuilderCapabilities {
    pub fn new(platform: impl Into<String>, trust_domain: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            features: BTreeSet::new(),
            resource_pools: BTreeSet::new(),
            capabilities: BTreeSet::new(),
            concurrency: 1,
            priority: 0,
            trust_domain: trust_domain.into(),
            cache_read: false,
            cache_write: false,
            execute: false,
        }
    }

    /// Derive the non-secret facts that are already present in a host binding.
    /// Action capabilities and optional features remain explicit because a
    /// binding must not silently grant more authority than its host record.
    pub fn from_binding(binding: &RemoteBuildBinding) -> Self {
        let mut capabilities = Self::new(&binding.platform, &binding.trust_domain);
        capabilities.resource_pools.insert(BuildResourcePool::CPU);
        capabilities.cache_read = binding.cache_read;
        capabilities.cache_write = binding.cache_write;
        capabilities.execute = binding.execute;
        capabilities
    }

    pub fn with_feature(mut self, feature: impl Into<String>) -> Self {
        self.features.insert(feature.into());
        self
    }

    pub fn with_pool(mut self, pool: BuildResourcePool) -> Self {
        self.resource_pools.insert(pool);
        self
    }

    pub fn with_capability(mut self, capability: BuildCapability) -> Self {
        self.capabilities.insert(capability);
        self
    }

    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_cache_read(mut self, enabled: bool) -> Self {
        self.cache_read = enabled;
        self
    }

    pub fn with_cache_write(mut self, enabled: bool) -> Self {
        self.cache_write = enabled;
        self
    }

    pub fn with_execute(mut self, enabled: bool) -> Self {
        self.execute = enabled;
        self
    }
}

/// One scheduler candidate: a host-owned binding plus its declared facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBuilder {
    pub binding: RemoteBuildBinding,
    pub capabilities: RemoteBuilderCapabilities,
}

impl RemoteBuilder {
    pub fn new(
        binding: RemoteBuildBinding,
        capabilities: RemoteBuilderCapabilities,
    ) -> Result<Self, RemoteScheduleError> {
        if binding.platform != capabilities.platform
            || binding.trust_domain != capabilities.trust_domain
            || binding.cache_read != capabilities.cache_read
            || binding.cache_write != capabilities.cache_write
            || binding.execute != capabilities.execute
        {
            return Err(RemoteScheduleError::BindingFactsMismatch {
                builder: binding.builder,
            });
        }
        Ok(Self {
            binding,
            capabilities,
        })
    }

    pub fn from_binding(binding: RemoteBuildBinding) -> Self {
        let capabilities = RemoteBuilderCapabilities::from_binding(&binding);
        Self {
            binding,
            capabilities,
        }
    }

    pub fn builder(&self) -> &str {
        &self.binding.builder
    }
}

/// A worker loss or transient transport failure that permits failover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAttemptError {
    pub retryable: bool,
    pub detail: String,
}

impl RemoteAttemptError {
    pub fn worker_lost(detail: impl Into<String>) -> Self {
        Self {
            retryable: true,
            detail: detail.into(),
        }
    }

    pub fn retryable(detail: impl Into<String>) -> Self {
        Self {
            retryable: true,
            detail: detail.into(),
        }
    }

    pub fn rejected(detail: impl Into<String>) -> Self {
        Self {
            retryable: false,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteScheduleError {
    DuplicateBuilder(String),
    BindingFactsMismatch {
        builder: String,
    },
    NoEligibleBuilder {
        action: ActionKey,
        fallback_local: bool,
    },
    CapacityExhausted {
        action: ActionKey,
        fallback_local: bool,
    },
    Rejected {
        action: ActionKey,
        builder: String,
        detail: String,
        fallback_local: bool,
    },
    AttemptsExhausted {
        action: ActionKey,
        attempted: Vec<String>,
        fallback_local: bool,
    },
}

impl fmt::Display for RemoteScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateBuilder(builder) => write!(f, "remote builder `{builder}` is duplicated"),
            Self::BindingFactsMismatch { builder } => write!(
                f,
                "remote builder `{builder}` binding facts do not match its capability record"
            ),
            Self::NoEligibleBuilder {
                action,
                fallback_local,
            } => write!(
                f,
                "no eligible remote builder for `{}` (local fallback: {fallback_local})",
                action.as_str()
            ),
            Self::CapacityExhausted {
                action,
                fallback_local,
            } => write!(
                f,
                "eligible remote builder capacity is exhausted for `{}` (local fallback: {fallback_local})",
                action.as_str()
            ),
            Self::Rejected {
                action,
                builder,
                detail,
                fallback_local,
            } => write!(
                f,
                "remote builder `{builder}` rejected `{}`: {detail} (local fallback: {fallback_local})",
                action.as_str()
            ),
            Self::AttemptsExhausted {
                action,
                attempted,
                fallback_local,
            } => write!(
                f,
                "remote builders exhausted for `{}` after [{}] (local fallback: {fallback_local})",
                action.as_str(),
                attempted.join(", ")
            ),
        }
    }
}

impl std::error::Error for RemoteScheduleError {}

#[derive(Debug, PartialEq, Eq)]
pub struct RemoteDispatch<T> {
    pub builder: String,
    pub attempted: Vec<String>,
    pub value: T,
}

/// Deterministic capability scheduler for host-owned builders.
#[derive(Debug, Clone, Default)]
pub struct RemoteScheduler {
    builders: BTreeMap<String, RemoteBuilder>,
    active: Arc<Mutex<BTreeMap<String, usize>>>,
    available: Arc<Condvar>,
}

impl RemoteScheduler {
    pub fn new<I>(builders: I) -> Result<Self, RemoteScheduleError>
    where
        I: IntoIterator<Item = RemoteBuilder>,
    {
        let mut registered = BTreeMap::new();
        for builder in builders {
            let name = builder.builder().to_string();
            if registered.insert(name.clone(), builder).is_some() {
                return Err(RemoteScheduleError::DuplicateBuilder(name));
            }
        }
        Ok(Self {
            builders: registered,
            active: Arc::default(),
            available: Arc::default(),
        })
    }

    pub fn builders(&self) -> impl Iterator<Item = &RemoteBuilder> {
        self.builders.values()
    }

    pub fn candidates<'a>(&'a self, request: &RemoteBuildRequest) -> Vec<&'a RemoteBuilder> {
        let mut candidates = self
            .builders
            .values()
            .filter(|builder| eligible(builder, request))
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .capabilities
                .priority
                .cmp(&left.capabilities.priority)
                .then_with(|| left.builder().cmp(right.builder()))
        });
        candidates
    }

    pub fn select(
        &self,
        request: &RemoteBuildRequest,
    ) -> Result<&RemoteBuilder, RemoteScheduleError> {
        self.candidates(request).into_iter().next().ok_or_else(|| {
            RemoteScheduleError::NoEligibleBuilder {
                action: request.action.clone(),
                fallback_local: request.fallback_local,
            }
        })
    }

    /// Try eligible builders in deterministic priority/name order. A retryable
    /// worker loss advances to the next candidate. The scheduler reports local
    /// fallback to its caller; it never executes a local action implicitly.
    pub fn dispatch<T, F>(
        &self,
        request: &RemoteBuildRequest,
        mut attempt: F,
    ) -> Result<RemoteDispatch<T>, RemoteScheduleError>
    where
        F: FnMut(&RemoteBuilder) -> Result<T, RemoteAttemptError>,
    {
        let candidates = self.candidates(request);
        if candidates.is_empty() {
            return Err(RemoteScheduleError::NoEligibleBuilder {
                action: request.action.clone(),
                fallback_local: request.fallback_local,
            });
        }
        let mut attempted = Vec::with_capacity(candidates.len());
        let mut reserved = false;
        for builder in candidates {
            let Some(reservation) = self.reserve(builder) else {
                continue;
            };
            reserved = true;
            let name = builder.builder().to_string();
            attempted.push(name.clone());
            let result = attempt(builder);
            drop(reservation);
            match result {
                Ok(value) => {
                    return Ok(RemoteDispatch {
                        builder: name,
                        attempted,
                        value,
                    });
                }
                Err(error) if error.retryable => {}
                Err(error) => {
                    return Err(RemoteScheduleError::Rejected {
                        action: request.action.clone(),
                        builder: name,
                        detail: error.detail,
                        fallback_local: request.fallback_local,
                    });
                }
            }
        }
        if !reserved {
            return Err(RemoteScheduleError::CapacityExhausted {
                action: request.action.clone(),
                fallback_local: request.fallback_local,
            });
        }
        Err(RemoteScheduleError::AttemptsExhausted {
            action: request.action.clone(),
            attempted,
            fallback_local: request.fallback_local,
        })
    }

    pub(super) fn acquire(&self, builder: &RemoteBuilder) -> RemoteBuilderLease {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            let count = active.entry(builder.builder().to_string()).or_default();
            if *count < builder.capabilities.concurrency {
                *count += 1;
                return RemoteBuilderLease {
                    active: Arc::clone(&self.active),
                    available: Arc::clone(&self.available),
                    builder: builder.builder().to_string(),
                };
            }
            active = self
                .available
                .wait(active)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    fn reserve(&self, builder: &RemoteBuilder) -> Option<RemoteBuilderLease> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = active.entry(builder.builder().to_string()).or_default();
        if *count >= builder.capabilities.concurrency {
            return None;
        }
        *count += 1;
        Some(RemoteBuilderLease {
            active: Arc::clone(&self.active),
            available: Arc::clone(&self.available),
            builder: builder.builder().to_string(),
        })
    }
}

pub(super) struct RemoteBuilderLease {
    active: Arc<Mutex<BTreeMap<String, usize>>>,
    available: Arc<Condvar>,
    builder: String,
}

impl Drop for RemoteBuilderLease {
    fn drop(&mut self) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = active.get_mut(&self.builder) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                active.remove(&self.builder);
            }
        }
        self.available.notify_one();
    }
}

fn eligible(builder: &RemoteBuilder, request: &RemoteBuildRequest) -> bool {
    let facts = &builder.capabilities;
    builder.binding.is_enabled()
        && facts.concurrency > 0
        && request
            .platform
            .as_ref()
            .is_none_or(|platform| platform == &facts.platform)
        && request
            .trust_domain
            .as_ref()
            .is_none_or(|trust| trust == &facts.trust_domain)
        && request.features.is_subset(&facts.features)
        && request.resource_pools.is_subset(&facts.resource_pools)
        && request.capabilities.is_subset(&facts.capabilities)
        && (!request.cache_read || facts.cache_read && builder.binding.cache_read)
        && (!request.cache_write || facts.cache_write && builder.binding.cache_write)
        && (!request.execute || facts.execute && builder.binding.execute)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(name: &str, execute: bool) -> RemoteBuildBinding {
        RemoteBuildBinding::new(name, format!("/var/lib/jet/{name}"), b"test-key")
            .unwrap()
            .with_trust_domain("trusted")
            .with_platform("linux-x86_64")
            .with_execute(execute)
    }

    fn capabilities(priority: i32, execute: bool) -> RemoteBuilderCapabilities {
        RemoteBuilderCapabilities::new("linux-x86_64", "trusted")
            .with_capability(BuildCapability::Exec)
            .with_feature("clang")
            .with_pool(BuildResourcePool::CPU)
            .with_concurrency(2)
            .with_priority(priority)
            .with_execute(execute)
    }

    fn request() -> RemoteBuildRequest {
        RemoteBuildRequest::new(ActionKey::new("remote-action"))
            .with_capability(BuildCapability::Exec)
            .with_platform("linux-x86_64")
            .with_trust_domain("trusted")
            .with_feature("clang")
            .with_pool(BuildResourcePool::CPU)
            .with_execute(true)
    }

    #[test]
    fn selects_priority_then_name() {
        let scheduler = RemoteScheduler::new([
            RemoteBuilder::new(binding("zeta", true), capabilities(10, true)).unwrap(),
            RemoteBuilder::new(binding("alpha", true), capabilities(10, true)).unwrap(),
            RemoteBuilder::new(binding("fast", true), capabilities(20, true)).unwrap(),
        ])
        .unwrap();

        assert_eq!(scheduler.select(&request()).unwrap().builder(), "fast");
        let tied = RemoteBuildRequest::new(ActionKey::new("remote-action"))
            .with_capability(BuildCapability::Exec)
            .with_platform("linux-x86_64")
            .with_trust_domain("trusted")
            .with_feature("clang")
            .with_pool(BuildResourcePool::CPU)
            .with_execute(true);
        let tied_scheduler = RemoteScheduler::new([
            RemoteBuilder::new(binding("zeta", true), capabilities(10, true)).unwrap(),
            RemoteBuilder::new(binding("alpha", true), capabilities(10, true)).unwrap(),
        ])
        .unwrap();
        assert_eq!(tied_scheduler.select(&tied).unwrap().builder(), "alpha");
    }

    #[test]
    fn retries_worker_loss_but_rejects_terminal_failure() {
        let scheduler = RemoteScheduler::new([
            RemoteBuilder::new(binding("fast", true), capabilities(20, true)).unwrap(),
            RemoteBuilder::new(binding("safe", true), capabilities(10, true)).unwrap(),
        ])
        .unwrap();
        let dispatched = scheduler
            .dispatch(&request(), |builder| {
                if builder.builder() == "fast" {
                    Err(RemoteAttemptError::worker_lost("worker disappeared"))
                } else {
                    Ok(builder.builder().to_string())
                }
            })
            .unwrap();
        assert_eq!(dispatched.builder, "safe");
        assert_eq!(dispatched.attempted, vec!["fast", "safe"]);
        assert_eq!(dispatched.value, "safe");

        let error = scheduler
            .dispatch(&request(), |_| {
                Err::<(), _>(RemoteAttemptError::rejected("bad result"))
            })
            .unwrap_err();
        assert!(matches!(
            error,
            RemoteScheduleError::Rejected { builder, .. } if builder == "fast"
        ));
    }

    #[test]
    fn dispatch_reserves_capacity_and_releases_it_after_completion() {
        use std::sync::mpsc;

        let scheduler = std::sync::Arc::new(
            RemoteScheduler::new([
                RemoteBuilder::new(
                    binding("fast", true),
                    capabilities(20, true).with_concurrency(1),
                )
                .unwrap(),
                RemoteBuilder::new(
                    binding("safe", true),
                    capabilities(10, true).with_concurrency(1),
                )
                .unwrap(),
            ])
            .unwrap(),
        );
        let request = request();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let first_scheduler = scheduler.clone();
            let first_request = request.clone();
            let first = scope.spawn(move || {
                first_scheduler
                    .dispatch(&first_request, |builder| {
                        assert_eq!(builder.builder(), "fast");
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok::<_, RemoteAttemptError>("first")
                    })
                    .unwrap()
            });

            started_rx.recv().unwrap();
            let second = scheduler
                .dispatch(&request, |builder| {
                    assert_eq!(builder.builder(), "safe");
                    Ok::<_, RemoteAttemptError>("second")
                })
                .unwrap();
            assert_eq!(second.builder, "safe");
            release_tx.send(()).unwrap();
            assert_eq!(first.join().unwrap().builder, "fast");

            let third = scheduler
                .dispatch(&request, |builder| {
                    assert_eq!(builder.builder(), "fast");
                    Ok::<_, RemoteAttemptError>("third")
                })
                .unwrap();
            assert_eq!(third.builder, "fast");
        });
    }

    #[test]
    fn separates_cache_and_execution_grants() {
        let binding = binding("cache-only", false).with_cache_read(true);
        let facts = RemoteBuilderCapabilities::from_binding(&binding);
        let scheduler =
            RemoteScheduler::new([RemoteBuilder::new(binding, facts).unwrap()]).unwrap();
        let cache_request = RemoteBuildRequest::new(ActionKey::new("remote-cache"))
            .with_platform("linux-x86_64")
            .with_trust_domain("trusted")
            .with_cache_read(true);
        assert_eq!(
            scheduler.select(&cache_request).unwrap().builder(),
            "cache-only"
        );
        assert!(matches!(
            scheduler.select(&request()),
            Err(RemoteScheduleError::NoEligibleBuilder { .. })
        ));
    }
}
