use automerge::{ReadDoc, transaction::Transactable};
use samod_test_harness::{Network, RunningDocIds};

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

/// When access is denied, `find_document` returns None (NotFound) on the
/// requesting side because the server sends DocUnavailable.
#[test]
fn denied_access_returns_not_found() {
    init_logging();
    let mut network = Network::new();

    let alice = network.create_samod("Alice");
    let bob = network.create_samod("Bob");

    // Alice denies all access and does not announce (no proactive push)
    network
        .samod(&alice)
        .set_announce_policy(Box::new(|_, _| false));
    network
        .samod(&alice)
        .set_access_policy(Box::new(|_doc_id, _peer_id| false));

    // Alice creates a document
    let RunningDocIds { doc_id, actor_id } = network.samod(&alice).create_document();

    network
        .samod(&alice)
        .with_document_by_actor(actor_id, |doc| {
            let mut tx = doc.transaction();
            tx.put(automerge::ROOT, "key", "value").unwrap();
            tx.commit();
        })
        .expect("with document should succeed");

    network.connect(alice, bob);
    network.run_until_quiescent();

    // Bob tries to find the document — should get None (NotFound)
    let bob_actor_id = network.samod(&bob).find_document(&doc_id);
    assert!(
        bob_actor_id.is_none(),
        "Bob should not be able to access Alice's document when access is denied"
    );
}

/// When access is allowed (default AllowAll behavior), sync proceeds normally.
#[test]
fn allowed_access_proceeds_normally() {
    init_logging();
    let mut network = Network::new();

    let alice = network.create_samod("Alice");
    let bob = network.create_samod("Bob");

    // Default access policy is AllowAll, so no need to set anything

    let RunningDocIds { doc_id, actor_id } = network.samod(&alice).create_document();

    network
        .samod(&alice)
        .with_document_by_actor(actor_id, |doc| {
            let mut tx = doc.transaction();
            tx.put(automerge::ROOT, "foo", "bar").unwrap();
            tx.commit();
        })
        .expect("with document should succeed");

    network.connect(alice, bob);
    network.run_until_quiescent();

    // Bob requests the document — should succeed
    let bob_actor_id = network
        .samod(&bob)
        .find_document(&doc_id)
        .expect("Bob should find Alice's document");

    let has_data = network
        .samod(&bob)
        .with_document_by_actor(bob_actor_id, |doc| {
            doc.get(automerge::ROOT, "foo")
                .unwrap()
                .map(|(v, _)| match v {
                    automerge::Value::Scalar(s) => match s.as_ref() {
                        automerge::ScalarValue::Str(string) => string == "bar",
                        _ => false,
                    },
                    _ => false,
                })
                .unwrap_or(false)
        })
        .expect("with document should succeed");

    assert!(has_data, "Bob should have Alice's data after allowed sync");
}

/// Access policy can selectively deny specific documents while allowing others.
/// Announce policy is set to not announce so Bob must explicitly request.
#[test]
fn selective_access_policy() {
    init_logging();
    let mut network = Network::new();

    let alice = network.create_samod("Alice");
    let bob = network.create_samod("Bob");

    // Alice creates two documents before setting policies
    let doc1 = network.samod(&alice).create_document();
    let doc2 = network.samod(&alice).create_document();

    network
        .samod(&alice)
        .with_document_by_actor(doc1.actor_id, |doc| {
            let mut tx = doc.transaction();
            tx.put(automerge::ROOT, "doc", "one").unwrap();
            tx.commit();
        })
        .unwrap();

    network
        .samod(&alice)
        .with_document_by_actor(doc2.actor_id, |doc| {
            let mut tx = doc.transaction();
            tx.put(automerge::ROOT, "doc", "two").unwrap();
            tx.commit();
        })
        .unwrap();

    // Alice does not announce (so Bob must request), and denies access to doc1
    network
        .samod(&alice)
        .set_announce_policy(Box::new(|_, _| false));
    let denied_doc_id = doc1.doc_id.clone();
    network
        .samod(&alice)
        .set_access_policy(Box::new(move |doc_id, _peer_id| doc_id != denied_doc_id));

    network.connect(alice, bob);
    network.run_until_quiescent();

    // Bob should NOT find doc1
    let bob_doc1 = network.samod(&bob).find_document(&doc1.doc_id);
    assert!(
        bob_doc1.is_none(),
        "Bob should not be able to access denied doc1"
    );

    // Bob SHOULD find doc2
    let bob_doc2 = network.samod(&bob).find_document(&doc2.doc_id);
    assert!(
        bob_doc2.is_some(),
        "Bob should be able to access allowed doc2"
    );
}

/// Regression for the outbound access gates (§5, §5b of the plan).
///
/// With the default `AlwaysAnnounce` announce policy, a document we hold is
/// proactively pushed to every connected peer. A denied peer must never be
/// associated with the document actor and so must never receive that proactive
/// `Sync`. Bob never requests the document (no `find_document`), so this
/// exercises the *outbound* direction only.
///
/// `connect_before_create` selects which association path is exercised:
/// - `true`  → connect Bob first, then create the doc → seeded via
///   `spawn_actor`'s `initial_connections` → **gate B** (§5b).
/// - `false` → create the doc first, then connect Bob → associated via
///   `ensure_connections` → **gate A** (§5).
fn denied_peer_receives_no_proactive_sync(connect_before_create: bool) {
    init_logging();
    let mut network = Network::new();

    let alice = network.create_samod("Alice");
    let bob = network.create_samod("Bob");

    // Alice denies Bob but leaves announce at the default `AlwaysAnnounce`, so
    // the only thing standing between Bob and the document is the access gate.
    network
        .samod(&alice)
        .set_access_policy(Box::new(|_doc_id, _peer_id| false));

    let doc_id = if connect_before_create {
        network.connect(alice, bob);
        network.run_until_quiescent();

        let RunningDocIds { doc_id, actor_id } = network.samod(&alice).create_document();
        network
            .samod(&alice)
            .with_document_by_actor(actor_id, |doc| {
                let mut tx = doc.transaction();
                tx.put(automerge::ROOT, "key", "value").unwrap();
                tx.commit();
            })
            .expect("with document should succeed");
        network.run_until_quiescent();
        doc_id
    } else {
        let RunningDocIds { doc_id, actor_id } = network.samod(&alice).create_document();
        network
            .samod(&alice)
            .with_document_by_actor(actor_id, |doc| {
                let mut tx = doc.transaction();
                tx.put(automerge::ROOT, "key", "value").unwrap();
                tx.commit();
            })
            .expect("with document should succeed");
        network.run_until_quiescent();

        network.connect(alice, bob);
        network.run_until_quiescent();
        doc_id
    };

    // Bob never requested the document; if the outbound gate held, no proactive
    // sync reached him, so he has no actor (and no data) for it.
    assert!(
        network.samod(&bob).document(&doc_id).is_none(),
        "Bob should not have received a proactive sync for a denied document"
    );
    assert_eq!(
        network.samod(&bob).actor_count(),
        0,
        "Bob should have spawned no document actor for a denied proactive sync"
    );
}

#[test]
fn denied_peer_receives_no_proactive_sync_on_spawn() {
    // connect first, then create → gate B (spawn_actor / initial_connections)
    denied_peer_receives_no_proactive_sync(true);
}

#[test]
fn denied_peer_receives_no_proactive_sync_after_spawn() {
    // create first, then connect → gate A (ensure_connections)
    denied_peer_receives_no_proactive_sync(false);
}

/// Regression for the ephemeral-broadcast leak that a `SendSyncMessage`-only
/// gate would miss. When Alice broadcasts an ephemeral message on a document
/// she holds, a denied peer connected to her must receive neither the
/// `Broadcast::New` nor any relayed gossip.
///
/// A third, allowed peer (Charlie) acts as a positive control: he must receive
/// the ephemeral, proving the broadcast actually fired.
///
/// `connect_before_create` selects the association path, as above (gate B when
/// true, gate A when false).
fn denied_peer_receives_no_ephemeral(connect_before_create: bool) {
    init_logging();
    let mut network = Network::new();

    let alice = network.create_samod("Alice");
    let bob = network.create_samod("Bob");
    let charlie = network.create_samod("Charlie");

    // Alice denies Bob only (Charlie is allowed) and leaves announce at the
    // default `AlwaysAnnounce`.
    let bob_peer = network.samod(&bob).peer_id();
    network
        .samod(&alice)
        .set_access_policy(Box::new(move |_doc_id, peer_id| peer_id != bob_peer));

    let (doc_id, alice_actor) = if connect_before_create {
        network.connect(alice, bob);
        network.connect(alice, charlie);
        network.run_until_quiescent();

        let RunningDocIds { doc_id, actor_id } = network.samod(&alice).create_document();
        network.run_until_quiescent();
        (doc_id, actor_id)
    } else {
        let RunningDocIds { doc_id, actor_id } = network.samod(&alice).create_document();
        network.run_until_quiescent();

        network.connect(alice, bob);
        network.connect(alice, charlie);
        network.run_until_quiescent();
        (doc_id, actor_id)
    };

    // Alice broadcasts an ephemeral message on the document.
    network.samod(&alice).broadcast(alice_actor, vec![1, 2, 3]);
    network.run_until_quiescent();

    // Positive control: Charlie (allowed) received the ephemeral via proactive
    // association.
    let charlie_actor = network
        .samod(&charlie)
        .find_document(&doc_id)
        .expect("Charlie should have the document");
    let charlie_msgs = network.samod(&charlie).pop_ephemera(charlie_actor);
    assert_eq!(
        charlie_msgs,
        vec![vec![1, 2, 3]],
        "the allowed peer should receive the broadcast ephemeral message"
    );

    // Bob (denied) was never associated with the actor, so he received neither
    // a proactive sync nor the ephemeral, and has no actor for the document.
    assert!(
        network.samod(&bob).document(&doc_id).is_none(),
        "the denied peer should not have received any traffic for the document"
    );
    assert_eq!(
        network.samod(&bob).actor_count(),
        0,
        "the denied peer should have spawned no document actor"
    );
}

#[test]
fn denied_peer_receives_no_ephemeral_on_spawn() {
    // connect first, then create → gate B (spawn_actor / initial_connections)
    denied_peer_receives_no_ephemeral(true);
}

#[test]
fn denied_peer_receives_no_ephemeral_after_spawn() {
    // create first, then connect → gate A (ensure_connections)
    denied_peer_receives_no_ephemeral(false);
}
