//! What a commit starts in other Sessions, the children it creates and the inputs it hands on,
//! checked in the sender's append transaction.
use crate::store::{accept, backend, sessions};
use enco_core::*;
use enco_kernel::{
    Accepted, Commit, CommitRejection, SessionDelivery, SessionDeliveryKind, StoreError,
};
use rusqlite::Transaction;

/// Create the children the commit's facts record, then deliver its inputs; returns the Sessions
/// whose Inbox accepted a new input.
///
/// A halt waiting for the sender stops it starting anything: a child, a message or a delegation
/// rejects the commit, and its boundaries are dropped while its facts still commit. Children are
/// created only past this check. An archive frees its subtree's names in the transaction that
/// leaves the halt, and an archived Session's actor commits nothing once it has taken that halt;
/// so a sender with no halt waiting is unarchived, and its actor alone names its children.
pub(super) fn send(
    tx: &Transaction<'_>,
    sender: SessionId,
    commit: &Commit,
    documents: Vec<String>,
) -> Result<Result<Vec<SessionId>, CommitRejection>, StoreError> {
    if halted(tx, sender)? {
        return Ok(if starts(commit) {
            Err(CommitRejection::Halted)
        } else {
            Ok(Vec::new())
        });
    }
    for creation in commit
        .entries
        .iter()
        .filter_map(|entry| entry.body.created_child())
    {
        sessions::create_child(tx, creation)?;
    }
    let mut delivered = Vec::new();
    for (delivery, document) in commit.deliveries.iter().zip(documents) {
        match deliver(tx, delivery, document)? {
            Delivered::Written => delivered.push(delivery.event.session),
            Delivered::Skipped => {}
            Delivered::Rejected(reason) => return Ok(Err(reason)),
        }
    }
    Ok(Ok(delivered))
}

/// Whether the commit starts something in another Session: a child, a message or a delegation.
fn starts(commit: &Commit) -> bool {
    commit
        .entries
        .iter()
        .any(|entry| entry.body.created_child().is_some())
        || commit
            .deliveries
            .iter()
            .any(|delivery| !matches!(delivery.kind, SessionDeliveryKind::Boundary))
}

/// Whether a halt still waits in the Session's Inbox once the commit has consumed its inputs.
fn halted(tx: &Transaction<'_>, session: SessionId) -> Result<bool, StoreError> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM inbox WHERE session_id=? AND json_extract(event, '$.body.kind')='halt')",
        [session.to_string()],
        |row| row.get(0),
    )
    .map_err(backend)
}

/// What became of one input a Session hands to another in its own transaction.
enum Delivered {
    /// The recipient's Inbox accepted it as new.
    Written,
    /// Nothing was written and the sender's facts still commit: a duplicate, or a boundary to an
    /// archived Session.
    Skipped,
    /// The whole transaction must not commit.
    Rejected(CommitRejection),
}

/// Check the recipient's side of one input and write it to the recipient's Inbox.
fn deliver(
    tx: &Transaction<'_>,
    delivery: &SessionDelivery,
    document: String,
) -> Result<Delivered, StoreError> {
    if let SessionDeliveryKind::Delegation {
        profile,
        working_dir,
    } = &delivery.kind
    {
        let child = sessions::get(tx, delivery.event.session)?
            .ok_or(StoreError::UnknownSession(delivery.event.session))?;
        if child.archived_at.is_some() {
            return Ok(Delivered::Rejected(CommitRejection::Archived(child.name)));
        }
        if let Some(requested) = profile
            && *requested != child.profile
        {
            return Ok(Delivered::Rejected(CommitRejection::ProfileMismatch {
                current: child.profile,
                requested: requested.clone(),
            }));
        }
        if let Some(requested) = working_dir
            && *requested != child.working_dir
        {
            return Ok(Delivered::Rejected(CommitRejection::WorkingDirMismatch {
                current: child.working_dir,
                requested: requested.clone(),
            }));
        }
    }
    let encoded = accept::Encoded {
        event: &delivery.event,
        document,
    };
    match accept::events_in(tx, std::slice::from_ref(&encoded))?[0] {
        Accepted::New => Ok(Delivered::Written),
        Accepted::Inactive => Err(StoreError::Inbox(format!(
            "delivery {} was refused as naming an inactive schedule",
            delivery.event.id
        ))),
        Accepted::Duplicate => Ok(Delivered::Skipped),
        Accepted::Archived => match delivery.kind {
            SessionDeliveryKind::Boundary => Ok(Delivered::Skipped),
            SessionDeliveryKind::Message | SessionDeliveryKind::Delegation { .. } => {
                let recipient = sessions::get(tx, delivery.event.session)?
                    .ok_or(StoreError::UnknownSession(delivery.event.session))?;
                Ok(Delivered::Rejected(CommitRejection::Archived(
                    recipient.name,
                )))
            }
        },
    }
}
