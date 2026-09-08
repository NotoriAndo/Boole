//! Out-of-band root-owned canary authority. The signing key never enters this
//! service. Missing configuration is an error, never an inferred opt-in.
use super::*;
use crate::fresh_answer_canary::{
    verify_grant, verify_redelivery, CanaryBindings, CanaryBudget, CanaryBudgetRole,
    CanaryTrustRoot, VerifiedCanaryGrant, VerifiedCanaryRedelivery,
};

pub struct InstalledCanaryAuthority {
    directory: File,
    root: CanaryTrustRoot,
    grant: VerifiedCanaryGrant,
    uid: u32,
    gid: u32,
}

pub struct InstalledCanaryRedeliveryAuthority {
    directory: File,
    root: CanaryTrustRoot,
    uid: u32,
    gid: u32,
}

impl InstalledCanaryRedeliveryAuthority {
    pub fn read(
        &self,
        grant: &VerifiedCanaryGrant,
    ) -> Result<VerifiedCanaryRedelivery, InstalledAuthorityError> {
        let bytes = read_bounded(&self.directory, "redelivery.json", 2048, self.uid, self.gid)?;
        let signature = read_bounded(&self.directory, "redelivery.sig", 64, self.uid, self.gid)?;
        Ok(verify_redelivery(&bytes, &signature, &self.root, grant)?)
    }
}

impl InstalledCanaryAuthority {
    pub fn grant(&self) -> &VerifiedCanaryGrant {
        &self.grant
    }

    pub fn into_parts(self) -> (VerifiedCanaryGrant, InstalledCanaryRedeliveryAuthority) {
        (
            self.grant,
            InstalledCanaryRedeliveryAuthority {
                directory: self.directory,
                root: self.root,
                uid: self.uid,
                gid: self.gid,
            },
        )
    }

    /// The run's directory is preprovisioned by the operator, not created by a
    /// request. Its immutable identifier is covered by the grant signature.
    pub fn open_budget(
        &self,
        role: CanaryBudgetRole,
        uid: u32,
        gid: u32,
    ) -> Result<(CanaryBudget, File), InstalledAuthorityError> {
        let root = filesystem_root()?;
        let directory = open_state_directory(&root, self.grant.journal_id(), role, uid, gid)?;
        let budget = CanaryBudget::open(&directory, &self.grant, role, uid, gid)?;
        Ok((budget, directory))
    }
}

pub fn open_installed_canary() -> Result<InstalledCanaryAuthority, InstalledAuthorityError> {
    open_beneath(&filesystem_root()?, 0, 0)
}

fn filesystem_root() -> Result<File, InstalledAuthorityError> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/")
        .map_err(|error| io_failure("/", error))
}

fn open_beneath(
    root: &File,
    uid: u32,
    gid: u32,
) -> Result<InstalledCanaryAuthority, InstalledAuthorityError> {
    let (parent, _) = open_verified_authority_directory(root, uid, gid)?;
    let directory = open_child(&parent, "development-canary-v1", true, "canary authority")?;
    validate_directory(&directory, "canary authority", uid, gid, Some(0o555))?;
    let key = read_bounded(&directory, "operator-public-key.bin", 32, uid, gid)?;
    let root = CanaryTrustRoot::new(
        key.try_into()
            .map_err(|_| unsafe_metadata("canary public key", "expected 32 bytes"))?,
    )?;
    let bytes = read_bounded(&directory, "grant.json", 16_384, uid, gid)?;
    let signature = read_bounded(&directory, "grant.sig", 64, uid, gid)?;
    let grant = verify_grant(&bytes, &signature, &root, &CanaryBindings::installed()?)?;
    Ok(InstalledCanaryAuthority {
        directory,
        root,
        grant,
        uid,
        gid,
    })
}

fn read_bounded(
    directory: &File,
    name: &str,
    cap: usize,
    uid: u32,
    gid: u32,
) -> Result<Vec<u8>, InstalledAuthorityError> {
    let file = open_child(directory, name, false, name)?;
    let len = file.metadata().map_err(|e| io_failure(name, e))?.len();
    if len == 0 || len > cap as u64 {
        return Err(unsafe_metadata(name, "canary file size"));
    }
    read_verified_file(file, name, uid, gid, len as usize)
}

fn open_state_directory(
    root: &File,
    journal_id: &str,
    role: CanaryBudgetRole,
    uid: u32,
    gid: u32,
) -> Result<File, InstalledAuthorityError> {
    validate_directory(root, "/", 0, 0, None)?;
    let mut current = root.try_clone().map_err(|e| io_failure("root", e))?;
    let role_directory = match role {
        CanaryBudgetRole::Node => "canary-node",
        CanaryBudgetRole::Launcher => "canary-launcher",
    };
    for component in ["var", "lib", "boole", "native-shadow", role_directory] {
        current = open_child(&current, component, true, component)?;
        validate_directory(&current, component, 0, 0, None)?;
    }
    let directory = open_child(&current, journal_id, true, "canary private run state")?;
    validate_directory(
        &directory,
        "canary private run state",
        uid,
        gid,
        Some(0o700),
    )?;
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fresh_answer_canary::{CanaryGrant, SIGNING_DOMAIN};
    use ed25519_dalek::{Signer, SigningKey};
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn installed_canary_requires_out_of_band_owned_key_and_regular_read_only_files() {
        let path = std::env::temp_dir().join(format!(
            "boole-installed-canary-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        let directory = path.join("usr/share/boole/native-shadow/development-canary-v1");
        std::fs::create_dir_all(&directory).unwrap();
        let key = SigningKey::from_bytes(&[47; 32]);
        let bytes = serde_json::to_vec(&CanaryGrant::one_task(
            "11".repeat(32),
            "22".repeat(32),
            10,
            CanaryBindings::installed().unwrap(),
        ))
        .unwrap();
        for (name, bytes) in [
            (
                "operator-public-key.bin",
                key.verifying_key().to_bytes().to_vec(),
            ),
            (
                "grant.sig",
                key.sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat())
                    .to_bytes()
                    .to_vec(),
            ),
            ("grant.json", bytes),
        ] {
            std::fs::write(directory.join(name), bytes).unwrap();
            std::fs::set_permissions(directory.join(name), std::fs::Permissions::from_mode(0o444))
                .unwrap();
        }
        std::fs::set_permissions(
            directory.parent().unwrap(),
            std::fs::Permissions::from_mode(0o555),
        )
        .unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
        let root = File::open(&path).unwrap();
        let meta = root.metadata().unwrap();
        assert!(open_beneath(&root, meta.uid(), meta.gid()).is_ok());
        std::fs::set_permissions(
            directory.join("operator-public-key.bin"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(open_beneath(&root, meta.uid(), meta.gid()).is_err());
        std::fs::set_permissions(
            directory.join("operator-public-key.bin"),
            std::fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(
            directory.join("operator-public-key.bin"),
            directory.join("original-key.bin"),
        )
        .unwrap();
        symlink(
            "original-key.bin",
            directory.join("operator-public-key.bin"),
        )
        .unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(open_beneath(&root, meta.uid(), meta.gid()).is_err());
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            directory.parent().unwrap(),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}
