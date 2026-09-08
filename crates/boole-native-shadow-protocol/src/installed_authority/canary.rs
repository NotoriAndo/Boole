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
    development_task: bool,
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
        let directory = open_state_directory(
            &root,
            self.grant.journal_id(),
            role,
            uid,
            gid,
            self.development_task,
        )?;
        let budget = CanaryBudget::open(&directory, &self.grant, role, uid, gid)?;
        Ok((budget, directory))
    }

    /// Re-read the root-owned signature and spec before each execution. A
    /// replaced configuration cannot adopt this process's already-open budget.
    pub fn reverify_task_materials(
        &self,
        materials: &mut VerifiedInstalledClosedLocalReplayExecutionMaterials,
    ) -> Result<(), InstalledAuthorityError> {
        if self.development_task {
            let bytes = read_bounded(&self.directory, "grant.json", 16_384, self.uid, self.gid)?;
            let signature = read_bounded(&self.directory, "grant.sig", 64, self.uid, self.gid)?;
            let verified = verify_selected_grant(&bytes, &signature, &self.root, true)?;
            if verified.digest() != self.grant.digest() {
                return Err(unsafe_metadata(
                    "development task grant",
                    "changed after startup",
                ));
            }
            materials.task = verified.task_bytes().to_vec();
            materials.anchor = verified.anchor_bytes().to_vec();
        }
        Ok(())
    }
}

pub fn open_installed_canary() -> Result<InstalledCanaryAuthority, InstalledAuthorityError> {
    open_beneath(&filesystem_root()?, 0, 0)
}

#[cfg(feature = "development-task-admission")]
pub fn open_installed_development_task() -> Result<InstalledCanaryAuthority, InstalledAuthorityError>
{
    open_selected_beneath(&filesystem_root()?, 0, 0, true)
}

fn verify_selected_grant(
    bytes: &[u8],
    signature: &[u8],
    root: &CanaryTrustRoot,
    development_task: bool,
) -> Result<VerifiedCanaryGrant, InstalledAuthorityError> {
    if development_task {
        #[cfg(feature = "development-task-admission")]
        return Ok(crate::fresh_answer_canary::development_task::verify_grant(
            bytes, signature, root,
        )?);
        #[cfg(not(feature = "development-task-admission"))]
        return Err(unsafe_metadata("development task", "feature disabled"));
    }
    Ok(verify_grant(
        bytes,
        signature,
        root,
        &CanaryBindings::installed()?,
    )?)
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
    open_selected_beneath(root, uid, gid, false)
}

fn open_selected_beneath(
    root: &File,
    uid: u32,
    gid: u32,
    development_task: bool,
) -> Result<InstalledCanaryAuthority, InstalledAuthorityError> {
    let (parent, _) = open_verified_authority_directory(root, uid, gid)?;
    let name = if development_task {
        "development-task-v1"
    } else {
        "development-canary-v1"
    };
    let directory = open_child(&parent, name, true, "canary authority")?;
    validate_directory(&directory, "canary authority", uid, gid, Some(0o555))?;
    let key = read_bounded(&directory, "operator-public-key.bin", 32, uid, gid)?;
    let root = CanaryTrustRoot::new(
        key.try_into()
            .map_err(|_| unsafe_metadata("canary public key", "expected 32 bytes"))?,
    )?;
    let bytes = read_bounded(&directory, "grant.json", 16_384, uid, gid)?;
    let signature = read_bounded(&directory, "grant.sig", 64, uid, gid)?;
    let grant = verify_selected_grant(&bytes, &signature, &root, development_task)?;
    Ok(InstalledCanaryAuthority {
        directory,
        root,
        grant,
        uid,
        gid,
        development_task,
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
    development_task: bool,
) -> Result<File, InstalledAuthorityError> {
    validate_directory(root, "/", 0, 0, None)?;
    let mut current = root.try_clone().map_err(|e| io_failure("root", e))?;
    let role_directory = match (development_task, role) {
        (false, CanaryBudgetRole::Node) => "canary-node",
        (false, CanaryBudgetRole::Launcher) => "canary-launcher",
        (true, CanaryBudgetRole::Node) => "development-task-node",
        (true, CanaryBudgetRole::Launcher) => "development-task-launcher",
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

    #[cfg(feature = "development-task-admission")]
    #[test]
    fn installed_development_task_is_separate_and_rejects_replacement_before_execution() {
        use crate::fresh_answer_canary::development_task::{
            DevelopmentTaskGrant, DevelopmentTaskSpec, SIGNING_DOMAIN,
        };
        let path = std::env::temp_dir().join(format!(
            "boole-installed-development-task-{}",
            std::process::id()
        ));
        let directory = path.join("usr/share/boole/native-shadow/development-task-v1");
        std::fs::create_dir_all(&directory).unwrap();
        let key = SigningKey::from_bytes(&[51; 32]);
        let make = |seed: &str| {
            serde_json::to_vec(
                &DevelopmentTaskGrant::one_task(
                    "11".repeat(32),
                    "22".repeat(32),
                    30,
                    DevelopmentTaskSpec {
                        type_name: "DevelopmentPoint".into(),
                        field_types: vec!["i32".into()],
                        task_seed: seed.repeat(32),
                        a0: 17,
                        mul: 3,
                        coeffs: vec![2],
                    },
                )
                .unwrap(),
            )
            .unwrap()
        };
        let provision = |bytes: &[u8]| {
            for (name, value) in [
                ("grant.json", bytes.to_vec()),
                (
                    "grant.sig",
                    key.sign(&[SIGNING_DOMAIN, bytes].concat()) // P2.10-exempt: disposable test key for the separate development task domain.
                        .to_bytes()
                        .to_vec(),
                ),
                (
                    "operator-public-key.bin",
                    key.verifying_key().to_bytes().to_vec(),
                ),
            ] {
                let file = directory.join(name);
                // The fixture owner replaces exact entries, not the runtime.
                if file.exists() {
                    std::fs::remove_file(&file).unwrap();
                }
                std::fs::write(&file, value).unwrap();
                std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o444)).unwrap();
            }
        };
        provision(&make("33"));
        std::fs::set_permissions(
            directory.parent().unwrap(),
            std::fs::Permissions::from_mode(0o555),
        )
        .unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
        let root = File::open(&path).unwrap();
        let metadata = root.metadata().unwrap();
        assert!(open_beneath(&root, metadata.uid(), metadata.gid()).is_err());
        let installed = open_selected_beneath(&root, metadata.uid(), metadata.gid(), true).unwrap();
        let mut materials = VerifiedInstalledClosedLocalReplayExecutionMaterials {
            task: vec![],
            anchor: vec![],
        };
        installed.reverify_task_materials(&mut materials).unwrap();
        assert_eq!(materials.task_bytes(), installed.grant().task_bytes());
        let before = materials.task_bytes().to_vec();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        provision(&make("44"));
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(installed.reverify_task_materials(&mut materials).is_err());
        assert_eq!(materials.task_bytes(), before);
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            directory.parent().unwrap(),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }

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
                key.sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat()) // P2.10-exempt: disposable test key for the development grant domain.
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
