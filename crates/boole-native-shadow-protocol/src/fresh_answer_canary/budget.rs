//! Durable at-most-once spending, distinct from the node's verdict journal.
//! A partial append is fatal. An existing empty file is never reinitialized.
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;

use serde::{Deserialize, Serialize};

use super::{identifier, CanaryError, VerifiedCanaryGrant, VerifiedCanaryRedelivery};
use crate::ExecutionRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanaryBudgetRole {
    Node,
    Launcher,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Open {
        grant_digest: String,
        journal_id: String,
        role: CanaryBudgetRole,
    },
    Candidate {
        candidate: String,
        submission: String,
    },
    Execute,
    Redeliver,
}

/// Neither cloneable nor serializable. Both node and launcher independently
/// spend their private journal before receiving this capability.
#[derive(Debug)]
pub struct VerifiedCanaryExecutionAuthorization {
    request: Box<ExecutionRequest>,
    materials: Option<std::sync::Arc<(Vec<u8>, Vec<u8>)>>,
}

impl VerifiedCanaryExecutionAuthorization {
    pub fn request(&self) -> &ExecutionRequest {
        &self.request
    }
    pub fn task_bytes(&self) -> &[u8] {
        self.materials.as_ref().map_or(
            crate::closed_local_replay_grant::TRACKED_REAL_HISTORY_TASK_BYTES,
            |m| m.0.as_slice(),
        )
    }
    pub fn anchor_bytes(&self) -> &[u8] {
        self.materials.as_ref().map_or(
            crate::closed_local_replay_grant::TRACKED_REAL_HISTORY_ANCHOR_BYTES,
            |m| m.1.as_slice(),
        )
    }
}

pub struct CanaryBudget {
    file: File,
    grant_digest: String,
    candidate: Option<(String, String)>,
    executed: bool,
    redelivered: bool,
    poisoned: bool,
}

impl CanaryBudget {
    pub fn open(
        directory: &File,
        grant: &VerifiedCanaryGrant,
        role: CanaryBudgetRole,
        uid: u32,
        gid: u32,
    ) -> Result<Self, CanaryError> {
        let metadata = directory.metadata()?;
        if !metadata.is_dir()
            || metadata.uid() != uid
            || metadata.gid() != gid
            || metadata.mode() & 0o7777 != 0o700
        {
            return Err(CanaryError::Rejected("private budget directory"));
        }
        let (file, created) = open_budget(directory, role)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != uid
            || metadata.gid() != gid
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
            || metadata.len() > 8192
        {
            return Err(CanaryError::Rejected("private budget file"));
        }
        lock(&file)?;
        let mut budget = Self {
            file,
            grant_digest: grant.digest().to_string(),
            candidate: None,
            executed: false,
            redelivered: false,
            poisoned: false,
        };
        if created {
            budget.append(&Event::Open {
                grant_digest: grant.digest().to_string(),
                journal_id: grant.journal_id().to_string(),
                role,
            })?;
            directory.sync_all()?;
        } else {
            let mut bytes = Vec::new();
            (&budget.file).take(8193).read_to_end(&mut bytes)?;
            if bytes.is_empty() || bytes.len() > 8192 || !bytes.ends_with(b"\n") {
                return Err(CanaryError::Rejected("incomplete budget journal"));
            }
            for (index, line) in bytes[..bytes.len() - 1].split(|b| *b == b'\n').enumerate() {
                crate::validate_strict_json(line)
                    .map_err(|_| CanaryError::Rejected("budget JSON"))?;
                match serde_json::from_slice::<Event>(line)? {
                    Event::Open {
                        grant_digest,
                        journal_id,
                        role: stored_role,
                    } if index == 0
                        && grant_digest == grant.digest()
                        && journal_id == grant.journal_id()
                        && stored_role == role => {}
                    Event::Candidate {
                        candidate,
                        submission,
                    } if index > 0
                        && budget.candidate.is_none()
                        && identifier(&candidate)
                        && identifier(&submission) =>
                    {
                        budget.candidate = Some((candidate, submission));
                    }
                    Event::Execute
                        if budget.candidate.is_some()
                            && !budget.executed
                            && !budget.redelivered =>
                    {
                        budget.executed = true;
                    }
                    Event::Redeliver if budget.candidate.is_some() && !budget.redelivered => {
                        budget.redelivered = true;
                    }
                    _ => return Err(CanaryError::Rejected("budget event order or authority")),
                }
                if index == 0 && budget.candidate.is_some() {
                    return Err(CanaryError::Rejected("budget missing header"));
                }
            }
        }
        Ok(budget)
    }

    fn append(&mut self, event: &Event) -> Result<(), CanaryError> {
        if self.poisoned {
            return Err(CanaryError::Rejected("poisoned budget"));
        }
        let mut bytes = serde_json::to_vec(event)?;
        bytes.push(b'\n');
        // Poison before touching the descriptor; no append can follow a partial
        // or ambiguously durable write even when the caller catches the error.
        self.poisoned = true;
        self.file.write_all(&bytes)?;
        self.file.sync_all()?;
        self.poisoned = false;
        Ok(())
    }

    pub fn admit_candidate(
        &mut self,
        grant: &VerifiedCanaryGrant,
        candidate: &str,
        submission: &str,
    ) -> Result<(), CanaryError> {
        self.check_grant(grant)?;
        if !identifier(candidate) || !identifier(submission) || self.candidate.is_some() {
            return Err(CanaryError::Rejected(
                "candidate already admitted or invalid",
            ));
        }
        self.append(&Event::Candidate {
            candidate: candidate.to_string(),
            submission: submission.to_string(),
        })?;
        self.candidate = Some((candidate.to_string(), submission.to_string()));
        Ok(())
    }

    pub fn matches_candidate(&self, candidate: &str, submission: &str) -> bool {
        !self.poisoned
            && self
                .candidate
                .as_ref()
                .is_some_and(|(c, s)| c == candidate && s == submission)
    }

    fn check_grant(&self, grant: &VerifiedCanaryGrant) -> Result<(), CanaryError> {
        if self.poisoned || self.grant_digest != grant.digest() {
            return Err(CanaryError::Rejected("budget authority or poison"));
        }
        Ok(())
    }

    pub fn reserve_execution(
        &mut self,
        grant: &VerifiedCanaryGrant,
        request: &ExecutionRequest,
    ) -> Result<VerifiedCanaryExecutionAuthorization, CanaryError> {
        self.check_grant(grant)?;
        grant.validate_request(request)?;
        if self.candidate.is_none() {
            self.admit_candidate(
                grant,
                request.candidate_digest_hex(),
                request.submission_digest_hex(),
            )?;
        }
        if self.executed
            || self.redelivered
            || !self.matches_candidate(
                request.candidate_digest_hex(),
                request.submission_digest_hex(),
            )
        {
            return Err(CanaryError::Rejected(
                "execution already spent or changed candidate",
            ));
        }
        self.append(&Event::Execute)?;
        self.executed = true;
        Ok(VerifiedCanaryExecutionAuthorization {
            request: Box::new(request.clone()),
            materials: grant.materials.clone(),
        })
    }

    /// Called only after the node found a durable terminal result. The signed
    /// recovery permission is not accepted as execution authority.
    pub fn consume_redelivery(
        &mut self,
        grant: &VerifiedCanaryGrant,
        permit: VerifiedCanaryRedelivery,
        candidate: &str,
        submission: &str,
    ) -> Result<(), CanaryError> {
        self.check_grant(grant)?;
        let recovery = permit.recovery;
        if self.redelivered
            || !self.matches_candidate(candidate, submission)
            || recovery.grant_digest != grant.digest()
            || recovery.candidate_digest != candidate
            || recovery.submission_digest != submission
        {
            return Err(CanaryError::Rejected(
                "redelivery spent or candidate mismatch",
            ));
        }
        self.append(&Event::Redeliver)?;
        self.redelivered = true;
        Ok(())
    }
}

#[allow(unsafe_code)]
fn open_budget(directory: &File, role: CanaryBudgetRole) -> Result<(File, bool), CanaryError> {
    let name = match role {
        CanaryBudgetRole::Node => c"node-budget-v1.jsonl",
        CanaryBudgetRole::Launcher => c"launcher-budget-v1.jsonl",
    };
    let flags =
        libc::O_RDWR | libc::O_APPEND | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK;
    // SAFETY: directory and static NUL-terminated name remain live across openat.
    let mut fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
    };
    let created = fd >= 0;
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EEXIST) {
            return Err(error.into());
        }
        // SAFETY: same verified directory and static filename; no create here.
        fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
    }
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful openat returned a new descriptor owned only here.
    Ok((unsafe { File::from_raw_fd(fd) }, created))
}

#[allow(unsafe_code)]
fn lock(file: &File) -> Result<(), CanaryError> {
    // SAFETY: flock only operates on the live descriptor and stores no pointer.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
