use std::sync::Arc;

use crate::{DocumentId, PeerId};

/// A synchronous access-control policy consulted by the [`Hub`](super::Hub) at
/// the actor↔peer boundary.
///
/// The closure returns `true` if the given peer may interact with the given
/// document at all. It is evaluated synchronously at the three association /
/// routing chokepoints in the hub, so a denied peer's data never reaches a
/// document actor (and hence the CRDT) and a denied peer is never associated
/// with an actor (and hence never sent any document traffic).
///
/// `Send + Sync` is required because the multi-threaded runtime shares the
/// [`Hub`](super::Hub) across tasks behind an `Arc<Mutex<…>>`.
pub(crate) type AccessPolicy = Arc<dyn Fn(&DocumentId, &PeerId) -> bool + Send + Sync>;

/// Returns the default [`AccessPolicy`] which allows every peer to interact
/// with every document.
pub(crate) fn allow_all() -> AccessPolicy {
    Arc::new(|_, _| true)
}
