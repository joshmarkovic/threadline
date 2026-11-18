use super::Receipt;
use crate::{AccountId, Error, ItemLocator, Result, Store};
use rusqlite::{OptionalExtension, params};

/// A backup is evidence of past local state, not of later remote effects. The caller must
/// supply authoritative evidence for the entire operation before making this decision.
pub enum RestoredDecision<'a> {
    /// No unrecorded remote effect occurred after the snapshot; older confirmed steps still stand.
    NoFurtherEffect { proof: &'a str },
    /// All intended remote effects have been confirmed by stable provider identities.
    Confirmed {
        proof: &'a str,
        receipts: &'a [Receipt],
    },
}

impl Store {
    pub fn reconcile_restored_operation(
        &self,
        account: AccountId,
        id: &str,
        decision: RestoredDecision<'_>,
    ) -> Result<()> {
        self.write(|tx| {
            let (recovery, previous, previous_reason, cancelled): (
                i64,
                String,
                Option<String>,
                bool,
            ) = tx
                .query_row("SELECT r.recovery_id,r.snapshot_state,r.snapshot_reason,o.cancel_requested
                 FROM restored_operations r JOIN operations o ON o.account_id=r.account_id AND o.id=r.operation_id
                 WHERE r.account_id=? AND r.operation_id=? AND r.decision IS NULL AND o.state='unknown'", params![account, id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })
                .optional()?
                .ok_or(Error::Conflict(
                    "restored operation has no open reconciliation",
                ))?;
            let (state, reason, decision_name, proof) = match decision {
                RestoredDecision::NoFurtherEffect { proof } => {
                    let (state, reason) = if cancelled {
                        ("cancelled", Some("cancel_remaining"))
                    } else if previous == "attention" {
                        ("attention", previous_reason.as_deref())
                    } else {
                        ("queued", Some("restored_no_further_effect"))
                    };
                    (state, reason, "no_further_effect", proof)
                }
                RestoredDecision::Confirmed { proof, receipts } => {
                    if receipts.is_empty() {
                        return Err(Error::InvalidInput(
                            "restored confirmation requires remote receipts",
                        ));
                    }
                    for receipt in receipts {
                        if [
                            &receipt.step,
                            &receipt.object_kind,
                            &receipt.remote_id,
                            &receipt.evidence,
                        ]
                        .into_iter()
                        .any(|s| s.trim().is_empty())
                        {
                            return Err(Error::InvalidInput("incomplete restored step receipt"));
                        }
                        let changed = tx.execute(
                            "INSERT INTO step_receipts VALUES(?,?,?,?,?,?) ON CONFLICT(account_id,operation_id,step)
                             DO UPDATE SET evidence=step_receipts.evidence WHERE step_receipts.object_kind=excluded.object_kind
                             AND step_receipts.remote_id=excluded.remote_id",
                            params![
                                account,
                                id,
                                receipt.step,
                                receipt.object_kind,
                                receipt.remote_id,
                                receipt.evidence
                            ],
                        )?;
                        if changed != 1 {
                            return Err(Error::Conflict(
                                "restored receipt contradicts retained remote identity",
                            ));
                        }
                    }
                    ("confirmed", Some("restored_confirmed"), "confirmed", proof)
                }
            };
            if proof.trim().is_empty() {
                return Err(Error::InvalidInput("reconciliation evidence is required"));
            }
            tx.execute("UPDATE restored_operations SET decision=?,evidence=? WHERE recovery_id=?", params![decision_name, proof, recovery])?;
            tx.execute("UPDATE operations SET state=?,reason=?,lease_until=NULL WHERE account_id=? AND id=?", params![state, reason, account, id])?;
            Ok(())
        })
    }

    /// Bind a fresh preflight decision to the caller's observed locator and the original stable item.
    /// If the item moves again, the stored decision no longer satisfies the dispatch gate.
    pub fn review_operation_locator(
        &self,
        account: AccountId,
        id: &str,
        verified: ItemLocator,
        evidence: &str,
    ) -> Result<()> {
        let number = i64::try_from(verified.number)
            .map_err(|_| Error::InvalidInput("verified locator number exceeds SQLite range"))?;
        if evidence.trim().is_empty() {
            return Err(Error::InvalidInput("verified locator evidence is required"));
        }
        self.write(|tx| {
            let valid: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM operations o JOIN items i ON i.id=o.item_id AND i.account_id=o.account_id
                 JOIN item_aliases a ON a.item_id=o.item_id AND a.account_id=o.account_id AND a.repository_id=o.original_repository
                 AND a.number=o.original_number WHERE o.account_id=?1 AND o.id=?2 AND i.repository_id=?3 AND i.number=?4)",
                params![account, id, verified.repository, number],
                |r| r.get(0),
            )?;
            if !valid {
                return Err(Error::Conflict(
                    "locator has no verified transfer from the operation target",
                ));
            }
            tx.execute(
                "INSERT INTO reviewed_operation_locators VALUES(?,?,?,?,?) ON CONFLICT(account_id,operation_id) DO UPDATE SET
                 repository_id=excluded.repository_id,number=excluded.number,evidence=excluded.evidence",
                params![account, id, verified.repository, number, evidence],
            )?;
            Ok(())
        })
    }
}
