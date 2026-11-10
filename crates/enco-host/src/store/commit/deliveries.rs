//! Preconditions for cross-Session inputs, checked in the sender's append transaction.
use crate::store::{accept, sessions};
use enco_kernel::{Accepted, CommitRejection, SessionDelivery, SessionDeliveryKind, StoreError};
use rusqlite::Transaction;

/// What became of one input a Session hands to another in its own transaction.
pub(super) enum Delivered {
    /// The recipient's Inbox accepted it as new.
    Written,
    /// Nothing was written and the sender's facts still commit: a duplicate, or a boundary a
    /// halt or archival blocks.
    Skipped,
    /// The whole transaction must not commit.
    Rejected(CommitRejection),
}

pub(super) fn deliver(
    tx: &Transaction<'_>,
    halted: bool,
    delivery: &SessionDelivery,
    document: String,
) -> Result<Delivered, StoreError> {
    match &delivery.kind {
        SessionDeliveryKind::Boundary if halted => return Ok(Delivered::Skipped),
        SessionDeliveryKind::Message | SessionDeliveryKind::Delegation { .. } if halted => {
            return Ok(Delivered::Rejected(CommitRejection::Halted));
        }
        SessionDeliveryKind::Delegation {
            name,
            create,
            profile,
            working_dir,
        } => {
            if let Some(creation) = create {
                if sessions::named(tx, &creation.name)?.is_some() {
                    return Ok(Delivered::Rejected(CommitRejection::ChildExists(
                        creation.name.clone(),
                    )));
                }
                sessions::create_child(tx, creation)?;
            } else {
                let Some(child) = sessions::get(tx, delivery.event.session)? else {
                    return Ok(Delivered::Rejected(CommitRejection::MissingProfile(
                        name.clone(),
                    )));
                };
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
        }
        SessionDeliveryKind::Boundary | SessionDeliveryKind::Message => {}
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
