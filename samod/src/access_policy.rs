use samod_core::{DocumentId, PeerId};

/// Whether to allow a peer to interact with a document at all.
///
/// To configure access control implement this trait and pass the
/// implementation to
/// [`RepoBuilder::with_access_policy`](crate::RepoBuilder::with_access_policy).
/// Note that the trait is implemented for `Fn(&DocumentId, &PeerId) -> bool`,
/// so a closure can be passed directly in many cases.
///
/// # Relationship to [`AnnouncePolicy`](crate::AnnouncePolicy)
///
/// Access policy is the strictly stronger control. Where an announce policy
/// only decides whether to *proactively offer* a document (a denied peer can
/// still request it by ID and receive it), an access policy decides whether a
/// peer may interact with the document *at all*: a denied peer is never
/// associated with the document's actor, so it is never announced to,
/// answered on request, or gossiped to.
///
/// # Evaluation model
///
/// The policy is consulted synchronously by the hub at the actor↔peer
/// boundary. It is evaluated per inbound message on the inbound side and once
/// per peer↔document association on the outbound side. It is set once when the
/// repository is built and cannot be changed afterwards, so it does not revoke
/// access for a peer that was already associated with a document before the
/// policy would deny it. Because it is synchronous it cannot perform async
/// work such as a database or remote-auth lookup.
///
/// # Authentication caveat
///
/// Note that the peer IDs are not authenticated by the network protocol
/// `samod` implements, so if you are relying on this method for authorization
/// you must make sure that the network layer you provide is doing
/// authentication in its own fashion somehow.
pub trait AccessPolicy: Send + Sync + 'static {
    /// Whether the given peer may interact with the given document.
    ///
    /// If this returns `false`, the peer's data for this document is never
    /// forwarded to the document actor and the peer is never associated with
    /// the actor, so it receives no traffic for the document. A peer that
    /// sends a `request` for a denied document receives a `doc-unavailable`
    /// response.
    fn is_allowed(&self, doc_id: &DocumentId, peer_id: &PeerId) -> bool;
}

impl<F> AccessPolicy for F
where
    F: Fn(&DocumentId, &PeerId) -> bool + Send + Sync + 'static,
{
    fn is_allowed(&self, doc_id: &DocumentId, peer_id: &PeerId) -> bool {
        self(doc_id, peer_id)
    }
}

/// Allow all peers to interact with all documents (the default behavior).
#[derive(Clone)]
pub struct AllowAll;

impl AccessPolicy for AllowAll {
    fn is_allowed(&self, _doc_id: &DocumentId, _peer_id: &PeerId) -> bool {
        true
    }
}
