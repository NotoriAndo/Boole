use thiserror::Error;

pub const NODE_ACCOUNT_NAME: &str = "boole-node";
pub const CHECKER_ACCOUNT_NAME: &str = "boole-native-checker";

#[cfg(any(target_os = "linux", test))]
const REQUIRED_HOME: &str = "/nonexistent";
#[cfg(any(target_os = "linux", test))]
const ALLOWED_SHELLS: [&str; 2] = ["/usr/sbin/nologin", "/bin/false"];

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct AccountRecord {
    name: String,
    uid: u32,
    gid: u32,
    home: String,
    shell: String,
}

#[cfg(test)]
impl AccountRecord {
    fn new(name: &str, uid: u32, gid: u32, home: &str, shell: &str) -> Self {
        Self {
            name: name.to_string(),
            uid,
            gid,
            home: home.to_string(),
            shell: shell.to_string(),
        }
    }
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupRecord {
    name: String,
    gid: u32,
}

#[cfg(test)]
impl GroupRecord {
    fn new(name: &str, gid: u32) -> Self {
        Self {
            name: name.to_string(),
            gid,
        }
    }
}

#[cfg(any(target_os = "linux", test))]
trait IdentityLookup {
    fn user(
        &mut self,
        name: &'static str,
    ) -> Result<Option<AccountRecord>, IdentityResolutionError>;
    fn group_by_name(
        &mut self,
        name: &'static str,
    ) -> Result<Option<GroupRecord>, IdentityResolutionError>;
    fn group_by_gid(&mut self, gid: u32) -> Result<Option<GroupRecord>, IdentityResolutionError>;
    fn groups(
        &mut self,
        name: &'static str,
        primary_gid: u32,
    ) -> Result<Vec<u32>, IdentityResolutionError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedServiceIdentity {
    uid: u32,
    gid: u32,
}

/// Fixed numeric identities independently resolved from the host NSS database.
///
/// Fields and construction remain private so callers cannot manufacture a
/// production identity binding from caller-selected numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedServiceIdentities {
    node: ResolvedServiceIdentity,
    checker: ResolvedServiceIdentity,
}

impl ResolvedServiceIdentities {
    pub fn node_uid(self) -> u32 {
        self.node.uid
    }

    pub fn node_gid(self) -> u32 {
        self.node.gid
    }

    pub fn checker_uid(self) -> u32 {
        self.checker.uid
    }

    pub fn checker_gid(self) -> u32 {
        self.checker.gid
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentityResolutionError {
    #[error("native-shadow fixed service identities require Linux")]
    UnsupportedPlatform,
    #[error("NSS lookup {operation} failed with code {code}")]
    Lookup { operation: &'static str, code: i32 },
    #[error("fixed service account {0} is missing")]
    MissingAccount(&'static str),
    #[error("fixed primary group {0} is missing")]
    MissingGroup(&'static str),
    #[error("fixed service identity contract failed for {subject}: {reason}")]
    Contract {
        subject: &'static str,
        reason: &'static str,
    },
    #[error("NSS returned invalid text for {field}")]
    InvalidText { field: &'static str },
    #[error("NSS lookup buffer capacity overflow")]
    BufferCapacity,
    #[error("NSS lookup buffer allocation failed")]
    BufferAllocation,
}

/// Resolve only the two account names frozen by the execution policy.
///
/// There is intentionally no caller-selected name or numeric-ID input.
pub fn resolve_fixed_service_identities(
) -> Result<ResolvedServiceIdentities, IdentityResolutionError> {
    #[cfg(target_os = "linux")]
    {
        let mut lookup = linux::LibcIdentityLookup;
        resolve_with_provider(&mut lookup)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(IdentityResolutionError::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "linux", test))]
fn resolve_with_provider<L: IdentityLookup>(
    lookup: &mut L,
) -> Result<ResolvedServiceIdentities, IdentityResolutionError> {
    let node = resolve_one(lookup, NODE_ACCOUNT_NAME)?;
    let checker = resolve_one(lookup, CHECKER_ACCOUNT_NAME)?;
    if node.uid == checker.uid {
        return Err(contract(NODE_ACCOUNT_NAME, "node and checker UIDs alias"));
    }
    if node.gid == checker.gid {
        return Err(contract(
            NODE_ACCOUNT_NAME,
            "node and checker primary GIDs alias",
        ));
    }
    Ok(ResolvedServiceIdentities { node, checker })
}

#[cfg(any(target_os = "linux", test))]
fn resolve_one<L: IdentityLookup>(
    lookup: &mut L,
    name: &'static str,
) -> Result<ResolvedServiceIdentity, IdentityResolutionError> {
    let account = lookup
        .user(name)?
        .ok_or(IdentityResolutionError::MissingAccount(name))?;
    if account.name != name {
        return Err(contract(
            name,
            "passwd name does not match the fixed account",
        ));
    }
    if account.uid == 0 || account.gid == 0 {
        return Err(contract(
            name,
            "account UID and primary GID must be non-root",
        ));
    }
    if account.home != REQUIRED_HOME {
        return Err(contract(name, "account home is not /nonexistent"));
    }
    if !ALLOWED_SHELLS.contains(&account.shell.as_str()) {
        return Err(contract(name, "account shell is not nologin or false"));
    }

    let named_group = lookup
        .group_by_name(name)?
        .ok_or(IdentityResolutionError::MissingGroup(name))?;
    if named_group.name != name || named_group.gid != account.gid {
        return Err(contract(
            name,
            "same-named group does not match the passwd primary GID",
        ));
    }
    let reverse_group = lookup
        .group_by_gid(account.gid)?
        .ok_or(IdentityResolutionError::MissingGroup(name))?;
    if reverse_group.name != name || reverse_group.gid != account.gid {
        return Err(contract(
            name,
            "primary GID does not resolve back to the same-named group",
        ));
    }

    let groups = lookup.groups(name, account.gid)?;
    if groups.as_slice() != [account.gid] {
        return Err(contract(
            name,
            "account must have exactly its primary GID and no supplementary groups",
        ));
    }

    Ok(ResolvedServiceIdentity {
        uid: account.uid,
        gid: account.gid,
    })
}

#[cfg(any(target_os = "linux", test))]
fn contract(subject: &'static str, reason: &'static str) -> IdentityResolutionError {
    IdentityResolutionError::Contract { subject, reason }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{CStr, CString};
    use std::mem::MaybeUninit;
    use std::os::raw::c_char;
    use std::ptr;

    use super::{contract, AccountRecord, GroupRecord, IdentityLookup, IdentityResolutionError};

    const FALLBACK_NSS_BUFFER_BYTES: usize = 1_024;

    pub(super) struct LibcIdentityLookup;

    impl IdentityLookup for LibcIdentityLookup {
        fn user(
            &mut self,
            name: &'static str,
        ) -> Result<Option<AccountRecord>, IdentityResolutionError> {
            lookup_user(name)
        }

        fn group_by_name(
            &mut self,
            name: &'static str,
        ) -> Result<Option<GroupRecord>, IdentityResolutionError> {
            lookup_group_by_name(name)
        }

        fn group_by_gid(
            &mut self,
            gid: u32,
        ) -> Result<Option<GroupRecord>, IdentityResolutionError> {
            lookup_group_by_gid(gid)
        }

        fn groups(
            &mut self,
            name: &'static str,
            primary_gid: u32,
        ) -> Result<Vec<u32>, IdentityResolutionError> {
            lookup_groups(name, primary_gid)
        }
    }

    #[allow(unsafe_code)]
    fn initial_buffer_size(key: libc::c_int) -> usize {
        // SAFETY: `sysconf` has no pointer arguments or memory side effects.
        let configured = unsafe { libc::sysconf(key) };
        usize::try_from(configured)
            .ok()
            .filter(|size| *size > 0)
            .unwrap_or(FALLBACK_NSS_BUFFER_BYTES)
    }

    fn allocate_buffer(size: usize) -> Result<Vec<u8>, IdentityResolutionError> {
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(size)
            .map_err(|_| IdentityResolutionError::BufferAllocation)?;
        buffer.resize(size, 0);
        Ok(buffer)
    }

    fn grow(size: usize) -> Result<usize, IdentityResolutionError> {
        size.checked_mul(2)
            .ok_or(IdentityResolutionError::BufferCapacity)
    }

    fn fixed_name(name: &'static str) -> CString {
        CString::new(name).expect("fixed account names contain no NUL")
    }

    #[allow(unsafe_code)]
    fn lookup_user(name: &'static str) -> Result<Option<AccountRecord>, IdentityResolutionError> {
        let name_c = fixed_name(name);
        let mut size = initial_buffer_size(libc::_SC_GETPW_R_SIZE_MAX);
        loop {
            let mut buffer = allocate_buffer(size)?;
            let mut record = MaybeUninit::<libc::passwd>::uninit();
            let mut result = ptr::null_mut();
            // SAFETY: all pointers reference live writable storage for this call;
            // returned string pointers are consumed before `buffer` is dropped.
            let status = unsafe {
                libc::getpwnam_r(
                    name_c.as_ptr(),
                    record.as_mut_ptr(),
                    buffer.as_mut_ptr().cast::<c_char>(),
                    buffer.len(),
                    &mut result,
                )
            };
            if status == libc::ERANGE {
                size = grow(size)?;
                continue;
            }
            if status != 0 {
                return Err(IdentityResolutionError::Lookup {
                    operation: "getpwnam_r",
                    code: status,
                });
            }
            if result.is_null() {
                return Ok(None);
            }
            // SAFETY: successful getpwnam_r returned `result` pointing at the
            // initialized record storage supplied above.
            let record = unsafe { record.assume_init() };
            return Ok(Some(AccountRecord {
                name: c_string(record.pw_name, "pw_name")?,
                uid: record.pw_uid,
                gid: record.pw_gid,
                home: c_string(record.pw_dir, "pw_dir")?,
                shell: c_string(record.pw_shell, "pw_shell")?,
            }));
        }
    }

    #[allow(unsafe_code)]
    fn lookup_group_by_name(
        name: &'static str,
    ) -> Result<Option<GroupRecord>, IdentityResolutionError> {
        let name_c = fixed_name(name);
        let mut size = initial_buffer_size(libc::_SC_GETGR_R_SIZE_MAX);
        loop {
            let mut buffer = allocate_buffer(size)?;
            let mut record = MaybeUninit::<libc::group>::uninit();
            let mut result = ptr::null_mut();
            // SAFETY: all pointers reference live writable storage for this call.
            let status = unsafe {
                libc::getgrnam_r(
                    name_c.as_ptr(),
                    record.as_mut_ptr(),
                    buffer.as_mut_ptr().cast::<c_char>(),
                    buffer.len(),
                    &mut result,
                )
            };
            if status == libc::ERANGE {
                size = grow(size)?;
                continue;
            }
            if status != 0 {
                return Err(IdentityResolutionError::Lookup {
                    operation: "getgrnam_r",
                    code: status,
                });
            }
            if result.is_null() {
                return Ok(None);
            }
            // SAFETY: successful getgrnam_r initialized the supplied record.
            let record = unsafe { record.assume_init() };
            return Ok(Some(GroupRecord {
                name: c_string(record.gr_name, "gr_name")?,
                gid: record.gr_gid,
            }));
        }
    }

    #[allow(unsafe_code)]
    fn lookup_group_by_gid(gid: u32) -> Result<Option<GroupRecord>, IdentityResolutionError> {
        let mut size = initial_buffer_size(libc::_SC_GETGR_R_SIZE_MAX);
        loop {
            let mut buffer = allocate_buffer(size)?;
            let mut record = MaybeUninit::<libc::group>::uninit();
            let mut result = ptr::null_mut();
            // SAFETY: all pointers reference live writable storage for this call.
            let status = unsafe {
                libc::getgrgid_r(
                    gid,
                    record.as_mut_ptr(),
                    buffer.as_mut_ptr().cast::<c_char>(),
                    buffer.len(),
                    &mut result,
                )
            };
            if status == libc::ERANGE {
                size = grow(size)?;
                continue;
            }
            if status != 0 {
                return Err(IdentityResolutionError::Lookup {
                    operation: "getgrgid_r",
                    code: status,
                });
            }
            if result.is_null() {
                return Ok(None);
            }
            // SAFETY: successful getgrgid_r initialized the supplied record.
            let record = unsafe { record.assume_init() };
            return Ok(Some(GroupRecord {
                name: c_string(record.gr_name, "gr_name")?,
                gid: record.gr_gid,
            }));
        }
    }

    #[allow(unsafe_code)]
    fn lookup_groups(
        name: &'static str,
        primary_gid: u32,
    ) -> Result<Vec<u32>, IdentityResolutionError> {
        let name_c = fixed_name(name);
        let mut groups = [primary_gid];
        let mut count: libc::c_int = 1;
        // SAFETY: `groups` has one writable gid_t slot and `count` advertises
        // exactly that capacity. The fixed username is NUL-terminated.
        let status = unsafe {
            libc::getgrouplist(
                name_c.as_ptr(),
                primary_gid,
                groups.as_mut_ptr(),
                &mut count,
            )
        };
        if status < 0 {
            if count > 1 {
                return Err(contract(
                    name,
                    "account has one or more supplementary groups",
                ));
            }
            return Err(IdentityResolutionError::Lookup {
                operation: "getgrouplist",
                code: std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
            });
        }
        if count != 1 {
            return Err(contract(
                name,
                "getgrouplist returned an invalid group count",
            ));
        }
        Ok(groups.to_vec())
    }

    #[allow(unsafe_code)]
    fn c_string(
        pointer: *const c_char,
        field: &'static str,
    ) -> Result<String, IdentityResolutionError> {
        if pointer.is_null() {
            return Err(IdentityResolutionError::InvalidText { field });
        }
        // SAFETY: NSS returned a NUL-terminated string pointer into the live
        // caller-owned lookup buffer. `to_owned` copies it before that buffer
        // is dropped.
        unsafe { CStr::from_ptr(pointer) }
            .to_str()
            .map(str::to_owned)
            .map_err(|_| IdentityResolutionError::InvalidText { field })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{
        resolve_with_provider, AccountRecord, GroupRecord, IdentityLookup, IdentityResolutionError,
        ResolvedServiceIdentities, CHECKER_ACCOUNT_NAME, NODE_ACCOUNT_NAME,
    };
    use crate::TRACKED_EXECUTION_POLICY_BYTES;

    #[derive(Clone)]
    struct FakeLookup {
        node: Option<AccountRecord>,
        checker: Option<AccountRecord>,
        node_group_by_name: Option<GroupRecord>,
        checker_group_by_name: Option<GroupRecord>,
        node_group_by_gid: Option<GroupRecord>,
        checker_group_by_gid: Option<GroupRecord>,
        node_groups: Vec<u32>,
        checker_groups: Vec<u32>,
        fail_operation: Option<&'static str>,
    }

    impl Default for FakeLookup {
        fn default() -> Self {
            Self {
                node: Some(AccountRecord::new(
                    NODE_ACCOUNT_NAME,
                    101,
                    201,
                    "/nonexistent",
                    "/usr/sbin/nologin",
                )),
                checker: Some(AccountRecord::new(
                    CHECKER_ACCOUNT_NAME,
                    102,
                    202,
                    "/nonexistent",
                    "/bin/false",
                )),
                node_group_by_name: Some(GroupRecord::new(NODE_ACCOUNT_NAME, 201)),
                checker_group_by_name: Some(GroupRecord::new(CHECKER_ACCOUNT_NAME, 202)),
                node_group_by_gid: Some(GroupRecord::new(NODE_ACCOUNT_NAME, 201)),
                checker_group_by_gid: Some(GroupRecord::new(CHECKER_ACCOUNT_NAME, 202)),
                node_groups: vec![201],
                checker_groups: vec![202],
                fail_operation: None,
            }
        }
    }

    impl IdentityLookup for FakeLookup {
        fn user(
            &mut self,
            name: &'static str,
        ) -> Result<Option<AccountRecord>, IdentityResolutionError> {
            self.maybe_fail("user")?;
            Ok(if name == NODE_ACCOUNT_NAME {
                self.node.clone()
            } else {
                self.checker.clone()
            })
        }

        fn group_by_name(
            &mut self,
            name: &'static str,
        ) -> Result<Option<GroupRecord>, IdentityResolutionError> {
            self.maybe_fail("group_by_name")?;
            Ok(if name == NODE_ACCOUNT_NAME {
                self.node_group_by_name.clone()
            } else {
                self.checker_group_by_name.clone()
            })
        }

        fn group_by_gid(
            &mut self,
            gid: u32,
        ) -> Result<Option<GroupRecord>, IdentityResolutionError> {
            self.maybe_fail("group_by_gid")?;
            Ok(if gid == 201 {
                self.node_group_by_gid.clone()
            } else {
                self.checker_group_by_gid.clone()
            })
        }

        fn groups(
            &mut self,
            name: &'static str,
            _primary_gid: u32,
        ) -> Result<Vec<u32>, IdentityResolutionError> {
            self.maybe_fail("groups")?;
            Ok(if name == NODE_ACCOUNT_NAME {
                self.node_groups.clone()
            } else {
                self.checker_groups.clone()
            })
        }
    }

    impl FakeLookup {
        fn maybe_fail(&self, operation: &'static str) -> Result<(), IdentityResolutionError> {
            if self.fail_operation == Some(operation) {
                return Err(IdentityResolutionError::Lookup { operation, code: 5 });
            }
            Ok(())
        }
    }

    fn resolve(
        lookup: &mut FakeLookup,
    ) -> Result<ResolvedServiceIdentities, IdentityResolutionError> {
        resolve_with_provider(lookup)
    }

    #[test]
    fn fixed_accounts_resolve_only_when_every_contract_field_matches() {
        let identities = resolve(&mut FakeLookup::default()).expect("valid fixed accounts");
        assert_eq!((identities.node_uid(), identities.node_gid()), (101, 201));
        assert_eq!(
            (identities.checker_uid(), identities.checker_gid()),
            (102, 202)
        );
    }

    #[test]
    fn missing_or_failed_nss_lookups_fail_closed() {
        let mut missing = FakeLookup {
            node: None,
            ..FakeLookup::default()
        };
        assert!(matches!(
            resolve(&mut missing),
            Err(IdentityResolutionError::MissingAccount(NODE_ACCOUNT_NAME))
        ));

        for operation in ["user", "group_by_name", "group_by_gid", "groups"] {
            let mut failed = FakeLookup {
                fail_operation: Some(operation),
                ..FakeLookup::default()
            };
            assert!(
                matches!(resolve(&mut failed), Err(IdentityResolutionError::Lookup { operation: actual, code: 5 }) if actual == operation)
            );
        }
    }

    #[test]
    fn root_aliases_wrong_profile_and_group_drift_are_rejected() {
        let mut cases = Vec::new();

        let mut root_uid = FakeLookup::default();
        root_uid.node.as_mut().unwrap().uid = 0;
        cases.push(root_uid);

        let mut root_gid = FakeLookup::default();
        root_gid.node.as_mut().unwrap().gid = 0;
        cases.push(root_gid);

        let mut wrong_home = FakeLookup::default();
        wrong_home.node.as_mut().unwrap().home = "/tmp".to_string();
        cases.push(wrong_home);

        let mut wrong_shell = FakeLookup::default();
        wrong_shell.node.as_mut().unwrap().shell = "/bin/sh".to_string();
        cases.push(wrong_shell);

        let mut wrong_named_gid = FakeLookup::default();
        wrong_named_gid.node_group_by_name.as_mut().unwrap().gid = 999;
        cases.push(wrong_named_gid);

        let mut wrong_reverse_name = FakeLookup::default();
        wrong_reverse_name.node_group_by_gid.as_mut().unwrap().name = "other".to_string();
        cases.push(wrong_reverse_name);

        let mut uid_alias = FakeLookup::default();
        uid_alias.checker.as_mut().unwrap().uid = 101;
        cases.push(uid_alias);

        let mut gid_alias = FakeLookup::default();
        gid_alias.checker.as_mut().unwrap().gid = 201;
        gid_alias.checker_group_by_name.as_mut().unwrap().gid = 201;
        gid_alias.checker_group_by_gid = Some(GroupRecord::new(CHECKER_ACCOUNT_NAME, 201));
        gid_alias.checker_groups = vec![201];
        cases.push(gid_alias);

        for mut case in cases {
            assert!(matches!(
                resolve(&mut case),
                Err(IdentityResolutionError::Contract { .. })
            ));
        }
    }

    #[test]
    fn zero_supplementary_groups_means_exactly_the_primary_gid_once() {
        for groups in [vec![], vec![201, 999], vec![201, 201]] {
            let mut lookup = FakeLookup {
                node_groups: groups,
                ..FakeLookup::default()
            };
            assert!(matches!(
                resolve(&mut lookup),
                Err(IdentityResolutionError::Contract { .. })
            ));
        }
    }

    #[test]
    fn fixed_names_and_profile_are_bound_to_the_tracked_policy() {
        let policy: Value =
            serde_json::from_slice(TRACKED_EXECUTION_POLICY_BYTES).expect("tracked policy JSON");
        assert_eq!(
            policy.pointer("/accounts/resolution"),
            Some(&Value::String(
                "getpwnam_r-and-getgrnam_r-at-launcher-start".to_string()
            ))
        );
        assert_eq!(
            policy.pointer("/accounts/userLookup"),
            Some(&Value::String("getpwnam_r".to_string()))
        );
        assert_eq!(
            policy.pointer("/accounts/groupLookup"),
            Some(&Value::String("getgrnam_r-and-getgrgid_r".to_string()))
        );
        assert_eq!(
            policy.pointer("/accounts/supplementaryGroupsLookup"),
            Some(&Value::String("getgrouplist".to_string()))
        );
        for field in [
            "primaryGroupMustMatchPasswdGid",
            "requireDistinctUid",
            "requireDistinctPrimaryGid",
            "numericIdsBoundForProcessLifetime",
        ] {
            assert_eq!(
                policy.pointer(&format!("/accounts/{field}")),
                Some(&Value::Bool(true))
            );
        }
        assert_eq!(
            policy.pointer("/accounts/node/name"),
            Some(&Value::String(NODE_ACCOUNT_NAME.to_string()))
        );
        assert_eq!(
            policy.pointer("/accounts/checker/name"),
            Some(&Value::String(CHECKER_ACCOUNT_NAME.to_string()))
        );
        for role in ["node", "checker"] {
            let name = if role == "node" {
                NODE_ACCOUNT_NAME
            } else {
                CHECKER_ACCOUNT_NAME
            };
            assert_eq!(
                policy.pointer(&format!("/accounts/{role}/primaryGroup")),
                Some(&Value::String(name.to_string()))
            );
            assert_eq!(
                policy.pointer(&format!("/accounts/{role}/requireNonRoot")),
                Some(&Value::Bool(true))
            );
            assert_eq!(
                policy.pointer(&format!("/accounts/{role}/home")),
                Some(&Value::String("/nonexistent".to_string()))
            );
            assert_eq!(
                policy.pointer(&format!("/accounts/{role}/allowedShells")),
                Some(&serde_json::json!(["/usr/sbin/nologin", "/bin/false"]))
            );
            assert_eq!(
                policy.pointer(&format!("/accounts/{role}/supplementaryGroupCount")),
                Some(&Value::from(0))
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "the named Ubuntu gate creates the two frozen service accounts"]
    fn real_fixed_accounts_resolve_in_named_linux_gate() {
        super::resolve_fixed_service_identities()
            .expect("fixed Linux service accounts must resolve");
    }
}
