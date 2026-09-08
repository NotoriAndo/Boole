//! Offline operator tool. No key generation, installation or runtime activation.
#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use boole_native_shadow_protocol::fresh_answer_canary::{
        verify_grant, verify_redelivery, CanaryBindings, CanaryGrant, CanaryRedelivery,
        CanaryTrustRoot, REDELIVERY_SIGNING_DOMAIN, SIGNING_DOMAIN,
    };
    use ed25519_dalek::{Signer, SigningKey};
    use std::{
        fs::{OpenOptions, Permissions},
        io::{Read, Write},
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        path::Path,
    };

    fn public_read(path: &Path, cap: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)?;
        if !file.metadata()?.is_file() || file.metadata()?.len() > cap {
            return Err("authority input is not a bounded regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(cap + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > cap {
            return Err("authority input exceeded cap".into());
        }
        Ok(bytes)
    }
    fn write_new(
        directory: &Path,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join(name))?;
        file.write_all(bytes)?;
        file.set_permissions(Permissions::from_mode(0o444))?;
        file.sync_all()?;
        Ok(())
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [command, key_path, first, second, third, output] = args.as_slice() else {
        return Err("usage: boole-canary-authority create-grant KEY_FILE RUN_ID JOURNAL_ID EPOCH OUTPUT_DIRECTORY | create-redelivery KEY_FILE GRANT_DIRECTORY CANDIDATE_DIGEST SUBMISSION_DIGEST OUTPUT_DIRECTORY".into());
    };
    if command != "create-grant" && command != "create-redelivery" {
        return Err("unknown operator command".into());
    }
    let key_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(key_path)?;
    let metadata = key_file.metadata()?;
    let uid = current_uid();
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != 0o600
        || metadata.uid() != uid
        || metadata.len() != 32
    {
        return Err("key file must be a private, singly linked, owned 32-byte seed".into());
    }
    let mut seed = zeroize::Zeroizing::new([0_u8; 32]);
    (&key_file).read_exact(&mut *seed)?;
    let key = SigningKey::from_bytes(&seed);
    drop(seed);
    let root = CanaryTrustRoot::new(key.verifying_key().to_bytes())?;
    let output = Path::new(output);
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(output)?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700 {
        return Err("output must be a precreated private directory owned by the operator".into());
    }
    let bindings = CanaryBindings::installed()?;
    if command == "create-grant" {
        let grant = CanaryGrant::one_task(
            first.clone(),
            second.clone(),
            third.parse()?,
            bindings.clone(),
        );
        let bytes = serde_json::to_vec(&grant)?;
        let signature = key.sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat());
        verify_grant(&bytes, &signature.to_bytes(), &root, &bindings)?;
        write_new(output, "grant.json", &bytes)?;
        write_new(output, "grant.sig", &signature.to_bytes())?;
        write_new(
            output,
            "operator-public-key.bin",
            &key.verifying_key().to_bytes(),
        )?;
    } else {
        let source = Path::new(first);
        let bytes = public_read(&source.join("grant.json"), 16_384)?;
        let signature = public_read(&source.join("grant.sig"), 64)?;
        let grant = verify_grant(&bytes, &signature, &root, &bindings)?;
        let bytes = serde_json::to_vec(&CanaryRedelivery::for_candidate(&grant, second, third))?;
        let signature = key.sign(&[REDELIVERY_SIGNING_DOMAIN, bytes.as_slice()].concat());
        verify_redelivery(&bytes, &signature.to_bytes(), &root, &grant)?;
        write_new(output, "redelivery.json", &bytes)?;
        write_new(output, "redelivery.sig", &signature.to_bytes())?;
    }
    directory.sync_all()?;
    println!("Created development-only authority; nothing installed or activated.");
    Ok(())
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn current_uid() -> u32 {
    // SAFETY: reads immutable process credentials; no pointers are involved.
    unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("canary authority tooling requires Unix private-file checks");
    std::process::exit(2);
}
