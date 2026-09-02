use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

#[cfg(target_os = "macos")]
use std::ffi::{OsStr, OsString};
#[cfg(target_os = "macos")]
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use boole_core::{
    GuestArtifactRole, ProductArtifactRole, VerifiedCurlProductRelease,
    VerifiedInstalledBootableCurlProductRelease,
};

const CONTROLLER_MAGIC: [u8; 8] = *b"BOOLE4C1";
const CONTROLLER_VERSION: u8 = 1;
const CONTROLLER_HEADER_BYTES: usize = 96;
pub(crate) const CONTROLLER_FRAME_CAP_BYTES: usize = 524_288;
const CONTROLLER_FRAME_COUNT_CAP: usize = 3;
const CONTROLLER_PAYLOAD_CAP_BYTES: usize =
    CONTROLLER_FRAME_COUNT_CAP * (CONTROLLER_FRAME_CAP_BYTES + 4);
#[cfg(target_os = "macos")]
const CONTROLLER_FAILURE_DIAGNOSTIC_CAP_BYTES: usize = 32 * 1024;
const CONTROLLER_CONTRACT_DIGEST: [u8; 32] = [
    0x98, 0x09, 0x5a, 0xbd, 0xe0, 0xcb, 0x32, 0xcb, 0x5f, 0xb2, 0x7e, 0xde, 0xaf, 0x5b, 0xc6, 0xc6,
    0x7f, 0x3d, 0xf7, 0x96, 0xad, 0x3c, 0xda, 0x07, 0xb1, 0x6f, 0x8b, 0x44, 0x84, 0xb9, 0xb7, 0x13,
];

#[cfg(test)]
fn boot_tuple_binding_hex(
    kernel_digest: &str,
    initrd_digest: &str,
    root_disk_digest: &str,
) -> Result<String, ControllerError> {
    let mut binding = Sha256::new();
    binding.update(b"boole.mac4.boot-tuple.v1\0");
    for digest in [kernel_digest, initrd_digest, root_disk_digest] {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ControllerError(
                "boot tuple contains a malformed sha256 digest".into(),
            ));
        }
        let bytes = hex::decode(digest)
            .map_err(|_| ControllerError("boot tuple digest cannot be decoded".into()))?;
        binding.update(bytes);
    }
    Ok(hex::encode(binding.finalize()))
}

fn direct_boot_binding_hex(
    kernel_digest: &str,
    root_disk_digest: &str,
) -> Result<String, ControllerError> {
    let mut binding = Sha256::new();
    binding.update(b"boole.mac4.boot-tuple.v2\0");
    for digest in [kernel_digest, root_disk_digest] {
        require_lowercase_sha256(digest, "direct boot tuple digest")?;
        binding.update(
            hex::decode(digest)
                .map_err(|_| ControllerError("boot tuple digest cannot be decoded".into()))?,
        );
    }
    Ok(hex::encode(binding.finalize()))
}

fn require_lowercase_sha256(value: &str, named: &str) -> Result<(), ControllerError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ControllerError(format!(
            "{named} is not a lowercase sha256 digest"
        )));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn bootable_controller_arguments(
    kernel: &Path,
    root_disk: &Path,
    console: &Path,
    receipt: &Path,
    kernel_digest: &str,
    root_disk_digest: &str,
    nonce_hex: &str,
    boot_binding_hex: &str,
) -> Result<Vec<OsString>, ControllerError> {
    require_lowercase_sha256(kernel_digest, "kernel digest")?;
    require_lowercase_sha256(root_disk_digest, "root disk digest")?;
    require_lowercase_sha256(nonce_hex, "boot nonce")?;
    require_lowercase_sha256(boot_binding_hex, "boot tuple binding")?;
    Ok(vec![
        "--kernel".into(),
        kernel.as_os_str().to_owned(),
        "--kernel-sha256".into(),
        kernel_digest.into(),
        "--root-disk".into(),
        root_disk.as_os_str().to_owned(),
        "--root-disk-sha256".into(),
        root_disk_digest.into(),
        "--cmdline".into(),
        "console=hvc0 root=/dev/vda ro init=/usr/lib/systemd/systemd".into(),
        "--nonce-hex".into(),
        nonce_hex.into(),
        "--boot-binding-hex".into(),
        boot_binding_hex.into(),
        "--console".into(),
        console.as_os_str().to_owned(),
        "--receipt".into(),
        receipt.as_os_str().to_owned(),
        "--timeout".into(),
        "115".into(),
    ])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControllerCommand {
    Qualification = 1,
    Execution = 2,
    Shutdown = 3,
}

impl ControllerCommand {
    fn response_kind(self) -> u8 {
        self as u8 | 0x80
    }

    fn request_frame_count(self) -> usize {
        match self {
            Self::Qualification => 1,
            Self::Execution => 2,
            Self::Shutdown => 0,
        }
    }

    fn response_frame_count(self) -> usize {
        match self {
            Self::Qualification => 2,
            Self::Execution => 3,
            Self::Shutdown => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ControllerLauncherPeer {
    pub(crate) pid: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
}

impl ControllerLauncherPeer {
    fn require_root(self) -> Result<Self, ControllerError> {
        if self.pid == 0 || self.uid != 0 || self.gid != 0 {
            return Err(ControllerError(
                "controller launcher peer is not root".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ControllerQualificationOutput {
    pub(crate) launcher_peer: ControllerLauncherPeer,
    pub(crate) proxy_ready_frame: Vec<u8>,
    pub(crate) launcher_ready_frame: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ControllerExecutionOutput {
    pub(crate) launcher_peer: ControllerLauncherPeer,
    pub(crate) proxy_ready_frame: Vec<u8>,
    pub(crate) launcher_ready_frame: Vec<u8>,
    pub(crate) launcher_report_frame: Vec<u8>,
}

#[derive(Debug)]
pub struct ControllerError(String);

impl fmt::Display for ControllerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ControllerError {}

#[derive(Debug)]
struct MaterializedControllerFile {
    runtime_directory: PathBuf,
    path: PathBuf,
    auxiliary_paths: Vec<PathBuf>,
}

impl MaterializedControllerFile {
    fn path(&self) -> &Path {
        &self.path
    }

    fn runtime_directory(&self) -> &Path {
        &self.runtime_directory
    }
}

impl Drop for MaterializedControllerFile {
    fn drop(&mut self) {
        for path in self.auxiliary_paths.iter().rev() {
            let _ = fs::remove_file(path);
        }
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.runtime_directory);
    }
}

fn materialize_verified_runtime_file(
    source: &File,
    expected_len: u64,
    expected_digest: &str,
    runtime_directory: &Path,
    basename: &str,
    mode: u32,
) -> Result<PathBuf, ControllerError> {
    if basename.is_empty()
        || basename == "."
        || basename == ".."
        || basename.contains('/')
        || !matches!(mode, 0o400 | 0o500)
    {
        return Err(ControllerError(
            "verified runtime target contract is invalid".into(),
        ));
    }
    if expected_digest.len() != 64
        || !expected_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ControllerError(
            "verified runtime digest is not lowercase sha256".into(),
        ));
    }
    let metadata = fs::symlink_metadata(runtime_directory)
        .map_err(|error| ControllerError(format!("inspect private runtime directory: {error}")))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(ControllerError(
            "verified runtime directory is not private 0700".into(),
        ));
    }
    let path = runtime_directory.join(basename);
    let result = (|| {
        let mut target = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|error| {
                ControllerError(format!("create verified runtime copy {basename}: {error}"))
            })?;
        let mut source = source.try_clone().map_err(|error| {
            ControllerError(format!("clone verified runtime handle {basename}: {error}"))
        })?;
        source.seek(SeekFrom::Start(0)).map_err(|error| {
            ControllerError(format!(
                "rewind verified runtime handle {basename}: {error}"
            ))
        })?;
        let mut digest = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = source.read(&mut buffer).map_err(|error| {
                ControllerError(format!("read verified runtime handle {basename}: {error}"))
            })?;
            if read == 0 {
                break;
            }
            copied = copied.checked_add(read as u64).ok_or_else(|| {
                ControllerError(format!("verified runtime length overflowed for {basename}"))
            })?;
            if copied > expected_len {
                return Err(ControllerError(format!(
                    "verified runtime byte length differs for {basename}"
                )));
            }
            digest.update(&buffer[..read]);
            target.write_all(&buffer[..read]).map_err(|error| {
                ControllerError(format!("write verified runtime copy {basename}: {error}"))
            })?;
        }
        if copied != expected_len {
            return Err(ControllerError(format!(
                "verified runtime byte length differs for {basename}"
            )));
        }
        if hex::encode(digest.finalize()) != expected_digest {
            return Err(ControllerError(format!(
                "verified runtime digest differs for {basename}"
            )));
        }
        target.sync_all().map_err(|error| {
            ControllerError(format!("sync verified runtime copy {basename}: {error}"))
        })?;
        target
            .set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|error| {
                ControllerError(format!("protect verified runtime copy {basename}: {error}"))
            })?;
        target.sync_all().map_err(|error| {
            ControllerError(format!("sync protected runtime copy {basename}: {error}"))
        })?;
        File::open(runtime_directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| ControllerError(format!("sync private runtime directory: {error}")))?;
        Ok(path.clone())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result
}

fn materialize_verified_controller_file(
    source: &File,
    expected_len: u64,
    expected_digest: &str,
    runtime_root: &Path,
) -> Result<MaterializedControllerFile, ControllerError> {
    let root_metadata = fs::symlink_metadata(runtime_root)
        .map_err(|error| ControllerError(format!("inspect controller runtime root: {error}")))?;
    if !root_metadata.is_dir()
        || root_metadata.file_type().is_symlink()
        || root_metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(ControllerError(
            "controller runtime root is not a private 0700 directory".into(),
        ));
    }

    let runtime_directory = runtime_root.join("active-controller");
    fs::create_dir(&runtime_directory).map_err(|error| {
        ControllerError(format!("create private controller directory: {error}"))
    })?;
    fs::set_permissions(&runtime_directory, fs::Permissions::from_mode(0o700)).map_err(
        |error| {
            let _ = fs::remove_dir(&runtime_directory);
            ControllerError(format!("protect private controller directory: {error}"))
        },
    )?;
    let materialized = MaterializedControllerFile {
        path: runtime_directory.join("host-controller"),
        runtime_directory,
        auxiliary_paths: Vec::new(),
    };

    materialize_verified_runtime_file(
        source,
        expected_len,
        expected_digest,
        materialized.runtime_directory(),
        "host-controller",
        0o500,
    )?;
    Ok(materialized)
}

#[cfg(any(target_os = "macos", test))]
fn read_bounded_runtime_diagnostic(path: &Path, cap: usize) -> std::io::Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "controller diagnostic is not a regular file",
        ));
    }
    let start = metadata.len().saturating_sub(cap as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((metadata.len() - start) as usize);
    file.take(cap as u64).read_to_end(&mut bytes)?;
    if start > 0 {
        if let Some(boundary) = bytes.iter().position(|byte| *byte == b'\n') {
            bytes.drain(..=boundary);
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(target_os = "macos")]
type SpawnedControllerClient = Mac4ControllerClient<ChildStdout, ChildStdin>;

/// One verified host-controller process materialized from the active release's
/// retained file handle.
///
/// The installed pathname is never reopened. The verified bytes are copied to
/// one private, fixed runtime directory and only that 0500 copy is executed.
/// The controller remains transport-only and confers no mining, reward,
/// consensus or activation authority.
#[cfg(target_os = "macos")]
pub struct SpawnedMac4Controller {
    client: Arc<SpawnedControllerClient>,
    child: Option<Child>,
    _materialized: MaterializedControllerFile,
}

#[cfg(target_os = "macos")]
impl SpawnedMac4Controller {
    #[allow(dead_code)]
    pub fn spawn(
        verified_release: &VerifiedCurlProductRelease,
        runtime_root: &Path,
        controller_arguments: &[OsString],
    ) -> Result<Self, ControllerError> {
        require_production_controller_arguments(controller_arguments)?;
        let role = ProductArtifactRole::HostController;
        let source = verified_release.artifact_file(role).ok_or_else(|| {
            ControllerError("verified active release lacks host-controller handle".into())
        })?;
        let expected_len = verified_release.artifact_byte_length(role).ok_or_else(|| {
            ControllerError("verified active release lacks host-controller byte length".into())
        })?;
        let expected_digest = verified_release.artifact_sha256(role).ok_or_else(|| {
            ControllerError("verified active release lacks host-controller digest".into())
        })?;
        let materialized = materialize_verified_controller_file(
            source,
            expected_len,
            expected_digest,
            runtime_root,
        )?;
        Self::spawn_materialized(materialized, controller_arguments)
    }

    /// Start the production controller using only bytes retained by the
    /// fully verified active product. Callers cannot supply kernel, disk or
    /// digest arguments, so an installed pathname swap cannot redirect the
    /// VM after verification.
    pub fn spawn_bootable_product(
        active: &VerifiedInstalledBootableCurlProductRelease,
        runtime_root: &Path,
    ) -> Result<Self, ControllerError> {
        let mut materialized = {
            let role = ProductArtifactRole::HostController;
            materialize_verified_controller_file(
                active.product().artifact_file(role).ok_or_else(|| {
                    ControllerError("verified active release lacks host-controller handle".into())
                })?,
                active.product().artifact_byte_length(role).ok_or_else(|| {
                    ControllerError(
                        "verified active release lacks host-controller byte length".into(),
                    )
                })?,
                active.product().artifact_sha256(role).ok_or_else(|| {
                    ControllerError("verified active release lacks host-controller digest".into())
                })?,
                runtime_root,
            )?
        };
        let guest_descriptor = |role| {
            let file = active.guest_artifact_file(role).ok_or_else(|| {
                ControllerError(format!(
                    "verified active guest lacks {} handle",
                    role.as_str()
                ))
            })?;
            let len = active.guest().artifact_byte_length(role).ok_or_else(|| {
                ControllerError(format!(
                    "verified active guest lacks {} byte length",
                    role.as_str()
                ))
            })?;
            let digest = active.guest().artifact_sha256(role).ok_or_else(|| {
                ControllerError(format!(
                    "verified active guest lacks {} digest",
                    role.as_str()
                ))
            })?;
            Ok::<_, ControllerError>((file, len, digest))
        };
        let (kernel_file, kernel_len, kernel_digest) =
            guest_descriptor(GuestArtifactRole::GuestKernel)?;
        let (root_file, root_len, root_digest) =
            guest_descriptor(GuestArtifactRole::GuestRootDisk)?;
        let kernel_path = materialize_verified_runtime_file(
            kernel_file,
            kernel_len,
            kernel_digest,
            materialized.runtime_directory(),
            "guest-kernel",
            0o400,
        )?;
        materialized.auxiliary_paths.push(kernel_path.clone());
        let root_path = materialize_verified_runtime_file(
            root_file,
            root_len,
            root_digest,
            materialized.runtime_directory(),
            "guest-root-disk",
            0o400,
        )?;
        materialized.auxiliary_paths.push(root_path.clone());
        let console_path = materialized.runtime_directory().join("guest-console.log");
        let receipt_path = materialized.runtime_directory().join("guest-receipt.json");
        materialized.auxiliary_paths.push(console_path.clone());
        materialized.auxiliary_paths.push(receipt_path.clone());
        let binding = direct_boot_binding_hex(kernel_digest, root_digest)?;
        let nonce = macos_fresh_nonce_hex()?;
        let arguments = bootable_controller_arguments(
            &kernel_path,
            &root_path,
            &console_path,
            &receipt_path,
            kernel_digest,
            root_digest,
            &nonce,
            &binding,
        )?;
        Self::spawn_materialized(materialized, &arguments)
    }

    fn spawn_materialized(
        materialized: MaterializedControllerFile,
        controller_arguments: &[OsString],
    ) -> Result<Self, ControllerError> {
        let mut child = Command::new(materialized.path())
            .args(controller_arguments)
            .arg("--controller-stdio")
            .current_dir(materialized.runtime_directory())
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| ControllerError(format!("spawn verified host-controller: {error}")))?;
        let (Some(stdout), Some(stdin)) = (child.stdout.take(), child.stdin.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ControllerError(
                "host-controller private stdio pipe absent".into(),
            ));
        };
        Ok(Self {
            client: Arc::new(Mac4ControllerClient::new(stdout, stdin)),
            child: Some(child),
            _materialized: materialized,
        })
    }

    pub fn client(&self) -> Arc<SpawnedControllerClient> {
        Arc::clone(&self.client)
    }

    /// Return bounded startup-only evidence before this private runtime is
    /// removed. This is called only when launcher qualification failed, before
    /// any submission bytes could reach the guest.
    pub fn failure_diagnostics(&self) -> String {
        let mut parts = Vec::new();
        for (name, label) in [
            ("guest-console.log", "guest-console-tail"),
            ("guest-receipt.json", "guest-receipt-tail"),
        ] {
            let path = self._materialized.runtime_directory().join(name);
            if let Ok(value) =
                read_bounded_runtime_diagnostic(&path, CONTROLLER_FAILURE_DIAGNOSTIC_CAP_BYTES)
            {
                parts.push(format!("{label}={value:?}"));
            }
        }
        if parts.is_empty() {
            "controller startup diagnostics unavailable".to_owned()
        } else {
            parts.join("; ")
        }
    }

    pub fn shutdown(mut self) -> Result<(), ControllerError> {
        if let Err(error) = self.client.shutdown() {
            self.kill_and_wait();
            return Err(error);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let Some(child) = self.child.as_mut() else {
                return Ok(());
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child.take();
                    if status.success() {
                        return Ok(());
                    }
                    return Err(ControllerError(format!(
                        "host-controller exited unsuccessfully: {status}"
                    )));
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    self.kill_and_wait();
                    return Err(ControllerError(
                        "host-controller did not exit after shutdown".into(),
                    ));
                }
                Err(error) => {
                    self.kill_and_wait();
                    return Err(ControllerError(format!(
                        "wait for host-controller failed: {error}"
                    )));
                }
            }
        }
    }

    fn kill_and_wait(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn macos_fresh_nonce_hex() -> Result<String, ControllerError> {
    let mut bytes = [0_u8; 32];
    // SAFETY: `bytes` is writable for exactly its live 32-byte extent. The
    // kernel neither retains the pointer nor aliases a Rust reference.
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err(ControllerError(format!(
            "generate fresh controller nonce: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(hex::encode(bytes))
}

#[cfg(target_os = "macos")]
impl Drop for SpawnedMac4Controller {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

#[cfg(target_os = "macos")]
#[cfg_attr(not(test), allow(dead_code))]
fn require_production_controller_arguments(arguments: &[OsString]) -> Result<(), ControllerError> {
    const FORBIDDEN: [&str; 4] = [
        "--controller-stdio",
        "--controller-protocol-dry-run",
        "--dry-run",
        "--proxy-dry-run",
    ];
    if arguments.iter().any(|argument| {
        FORBIDDEN
            .iter()
            .any(|forbidden| argument == OsStr::new(forbidden))
    }) {
        return Err(ControllerError(
            "controller arguments contain a forbidden mode override".into(),
        ));
    }
    Ok(())
}

struct ControllerIo<R, W> {
    reader: R,
    writer: W,
    stopped: bool,
}

/// Serialized node-side owner of one persistent Mac host-controller stream.
///
/// Construct this only from the private stdin/stdout pipes of the verified
/// `host-controller` artifact. It is transport plumbing and owns no verdict,
/// challenge, journal, reward, consensus or activation authority.
pub struct Mac4ControllerClient<R, W> {
    io: Mutex<ControllerIo<R, W>>,
}

impl<R, W> fmt::Debug for Mac4ControllerClient<R, W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Mac4ControllerClient")
            .finish_non_exhaustive()
    }
}

impl<R: Read, W: Write> Mac4ControllerClient<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            io: Mutex::new(ControllerIo {
                reader,
                writer,
                stopped: false,
            }),
        }
    }

    pub(crate) fn qualify(
        &self,
        launcher_hello_frame: &[u8],
    ) -> Result<ControllerQualificationOutput, ControllerError> {
        let response = self.transact(ControllerCommand::Qualification, &[launcher_hello_frame])?;
        let [proxy_ready_frame, launcher_ready_frame]: [Vec<u8>; 2] = response
            .frames
            .try_into()
            .map_err(|_| ControllerError("qualification frame count differs".into()))?;
        Ok(ControllerQualificationOutput {
            launcher_peer: response
                .launcher_peer
                .ok_or_else(|| ControllerError("qualification launcher peer absent".into()))?,
            proxy_ready_frame,
            launcher_ready_frame,
        })
    }

    pub(crate) fn execute(
        &self,
        launcher_hello_frame: &[u8],
        launcher_request_frame: &[u8],
    ) -> Result<ControllerExecutionOutput, ControllerError> {
        let response = self.transact(
            ControllerCommand::Execution,
            &[launcher_hello_frame, launcher_request_frame],
        )?;
        let [proxy_ready_frame, launcher_ready_frame, launcher_report_frame]: [Vec<u8>; 3] =
            response
                .frames
                .try_into()
                .map_err(|_| ControllerError("execution frame count differs".into()))?;
        Ok(ControllerExecutionOutput {
            launcher_peer: response
                .launcher_peer
                .ok_or_else(|| ControllerError("execution launcher peer absent".into()))?,
            proxy_ready_frame,
            launcher_ready_frame,
            launcher_report_frame,
        })
    }

    pub fn shutdown(&self) -> Result<(), ControllerError> {
        self.transact(ControllerCommand::Shutdown, &[])?;
        Ok(())
    }

    fn transact(
        &self,
        command: ControllerCommand,
        frames: &[&[u8]],
    ) -> Result<DecodedEnvelope, ControllerError> {
        let mut io = self
            .io
            .lock()
            .map_err(|_| ControllerError("controller I/O lock is poisoned".into()))?;
        if io.stopped {
            return Err(ControllerError("controller is already stopped".into()));
        }
        if frames.len() != command.request_frame_count() {
            return Err(ControllerError(
                "controller request frame count differs".into(),
            ));
        }
        let id = request_id(command, frames);
        let request = encode_envelope(command as u8, id, None, frames)?;
        io.writer
            .write_all(&request)
            .and_then(|()| io.writer.flush())
            .map_err(|error| ControllerError(format!("write controller request: {error}")))?;
        let response = read_envelope(&mut io.reader)?;
        if response.kind != command.response_kind() {
            return Err(ControllerError("controller response kind differs".into()));
        }
        if response.request_id != id {
            return Err(ControllerError(
                "controller response request binding differs".into(),
            ));
        }
        if response.frames.len() != command.response_frame_count() {
            return Err(ControllerError(
                "controller response frame count differs".into(),
            ));
        }
        match command {
            ControllerCommand::Qualification | ControllerCommand::Execution => {
                response
                    .launcher_peer
                    .ok_or_else(|| ControllerError("controller launcher peer absent".into()))?
                    .require_root()?;
            }
            ControllerCommand::Shutdown => {
                if response.launcher_peer.is_some() {
                    return Err(ControllerError(
                        "shutdown response forged launcher peer".into(),
                    ));
                }
                io.stopped = true;
            }
        }
        Ok(response)
    }

    #[cfg(test)]
    fn into_inner(self) -> Result<(R, W), ControllerError> {
        let inner = self
            .io
            .into_inner()
            .map_err(|_| ControllerError("controller I/O lock is poisoned".into()))?;
        Ok((inner.reader, inner.writer))
    }
}

#[derive(Debug)]
struct DecodedEnvelope {
    kind: u8,
    request_id: [u8; 32],
    launcher_peer: Option<ControllerLauncherPeer>,
    frames: Vec<Vec<u8>>,
}

pub(crate) fn request_id(command: ControllerCommand, frames: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update([command as u8]);
    for frame in frames {
        digest.update((frame.len() as u32).to_be_bytes());
        digest.update(frame);
    }
    digest.finalize().into()
}

fn encode_envelope(
    kind: u8,
    request_id: [u8; 32],
    launcher_peer: Option<ControllerLauncherPeer>,
    frames: &[&[u8]],
) -> Result<Vec<u8>, ControllerError> {
    if frames.len() > CONTROLLER_FRAME_COUNT_CAP {
        return Err(ControllerError("controller frame count exceeds cap".into()));
    }
    let mut payload = Vec::new();
    for frame in frames {
        if frame.len() > CONTROLLER_FRAME_CAP_BYTES {
            return Err(ControllerError("controller frame exceeds cap".into()));
        }
        payload.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        payload.extend_from_slice(frame);
    }
    if payload.len() > CONTROLLER_PAYLOAD_CAP_BYTES {
        return Err(ControllerError("controller payload exceeds cap".into()));
    }
    let mut output = vec![0_u8; CONTROLLER_HEADER_BYTES];
    output[..8].copy_from_slice(&CONTROLLER_MAGIC);
    output[8] = CONTROLLER_VERSION;
    output[9] = kind;
    output[10..12].copy_from_slice(&(frames.len() as u16).to_be_bytes());
    output[12..16].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    output[16..48].copy_from_slice(&request_id);
    output[48..80].copy_from_slice(&CONTROLLER_CONTRACT_DIGEST);
    if let Some(peer) = launcher_peer {
        output[80..84].copy_from_slice(&peer.pid.to_be_bytes());
        output[84..88].copy_from_slice(&peer.uid.to_be_bytes());
        output[88..92].copy_from_slice(&peer.gid.to_be_bytes());
    }
    output.extend_from_slice(&payload);
    Ok(output)
}

fn read_envelope(reader: &mut impl Read) -> Result<DecodedEnvelope, ControllerError> {
    let mut header = [0_u8; CONTROLLER_HEADER_BYTES];
    reader
        .read_exact(&mut header)
        .map_err(|error| ControllerError(format!("read controller header: {error}")))?;
    if header[..8] != CONTROLLER_MAGIC
        || header[8] != CONTROLLER_VERSION
        || header[48..80] != CONTROLLER_CONTRACT_DIGEST
        || header[92..96] != [0, 0, 0, 0]
    {
        return Err(ControllerError("controller header identity differs".into()));
    }
    let frame_count = u16::from_be_bytes([header[10], header[11]]) as usize;
    let payload_len =
        u32::from_be_bytes(header[12..16].try_into().expect("fixed header range")) as usize;
    if frame_count > CONTROLLER_FRAME_COUNT_CAP || payload_len > CONTROLLER_PAYLOAD_CAP_BYTES {
        return Err(ControllerError("controller response exceeds cap".into()));
    }
    let mut request_id = [0_u8; 32];
    request_id.copy_from_slice(&header[16..48]);
    let pid = u32::from_be_bytes(header[80..84].try_into().expect("fixed peer range"));
    let uid = u32::from_be_bytes(header[84..88].try_into().expect("fixed peer range"));
    let gid = u32::from_be_bytes(header[88..92].try_into().expect("fixed peer range"));
    let launcher_peer = if (pid, uid, gid) == (0, 0, 0) {
        None
    } else {
        Some(ControllerLauncherPeer { pid, uid, gid })
    };
    let mut payload = vec![0_u8; payload_len];
    reader
        .read_exact(&mut payload)
        .map_err(|error| ControllerError(format!("read controller payload: {error}")))?;
    let mut offset = 0;
    let mut frames = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        if payload.len().saturating_sub(offset) < 4 {
            return Err(ControllerError(
                "controller frame header is truncated".into(),
            ));
        }
        let length = u32::from_be_bytes(
            payload[offset..offset + 4]
                .try_into()
                .expect("checked frame header"),
        ) as usize;
        offset += 4;
        if length > CONTROLLER_FRAME_CAP_BYTES || payload.len().saturating_sub(offset) < length {
            return Err(ControllerError(
                "controller frame is truncated or oversized".into(),
            ));
        }
        frames.push(payload[offset..offset + length].to_vec());
        offset += length;
    }
    if offset != payload.len() {
        return Err(ControllerError(
            "controller payload has trailing bytes".into(),
        ));
    }
    Ok(DecodedEnvelope {
        kind: header[9],
        request_id,
        launcher_peer,
        frames,
    })
}

#[cfg(test)]
pub(crate) fn encode_response_for_test(
    command: ControllerCommand,
    request_id: [u8; 32],
    launcher_peer: Option<ControllerLauncherPeer>,
    frames: &[&[u8]],
) -> Vec<u8> {
    encode_envelope(command.response_kind(), request_id, launcher_peer, frames)
        .expect("test response")
}

#[cfg(test)]
fn encode_request_for_test(
    command: ControllerCommand,
    frames: &[&[u8]],
) -> Result<Vec<u8>, ControllerError> {
    encode_envelope(command as u8, request_id(command, frames), None, frames)
}

#[cfg(test)]
fn decode_request_kinds(mut bytes: &[u8]) -> Vec<u8> {
    let mut kinds = Vec::new();
    while !bytes.is_empty() {
        let before = bytes.len();
        let decoded = read_envelope(&mut bytes).expect("request envelope");
        kinds.push(decoded.kind);
        assert!(bytes.len() < before);
    }
    kinds
}

#[cfg(test)]
mod tests {
    use super::{
        encode_response_for_test, ControllerCommand, ControllerLauncherPeer, Mac4ControllerClient,
    };
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::io::{Cursor, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct FixtureDirectory(std::path::PathBuf);

    impl FixtureDirectory {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "boole-mac4-controller-{label}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create fixture directory");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("set private fixture mode");
            Self(path)
        }
    }

    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn persistent_controller_qualifies_once_and_serves_multiple_executions() {
        let peer = ControllerLauncherPeer {
            pid: 4242,
            uid: 0,
            gid: 0,
        };
        let qualification_ready = b"qualification-ready".to_vec();
        let execution_ready_one = b"execution-ready-one".to_vec();
        let execution_report_one = b"execution-report-one".to_vec();
        let execution_ready_two = b"execution-ready-two".to_vec();
        let execution_report_two = b"execution-report-two".to_vec();
        let qualification_id = super::request_id(ControllerCommand::Qualification, &[b"q"]);
        let execution_one_id = super::request_id(ControllerCommand::Execution, &[b"h1", b"r1"]);
        let execution_two_id = super::request_id(ControllerCommand::Execution, &[b"h2", b"r2"]);
        let shutdown_id = super::request_id(ControllerCommand::Shutdown, &[]);
        let input = [
            encode_response_for_test(
                ControllerCommand::Qualification,
                qualification_id,
                Some(peer),
                &[b"proxy-q".as_slice(), qualification_ready.as_slice()],
            ),
            encode_response_for_test(
                ControllerCommand::Execution,
                execution_one_id,
                Some(peer),
                &[
                    b"proxy-e1".as_slice(),
                    execution_ready_one.as_slice(),
                    execution_report_one.as_slice(),
                ],
            ),
            encode_response_for_test(
                ControllerCommand::Execution,
                execution_two_id,
                Some(peer),
                &[
                    b"proxy-e2".as_slice(),
                    execution_ready_two.as_slice(),
                    execution_report_two.as_slice(),
                ],
            ),
            encode_response_for_test(ControllerCommand::Shutdown, shutdown_id, None, &[]),
        ]
        .concat();
        let reader = Cursor::new(input);
        let writer = Vec::<u8>::new();
        let controller = Mac4ControllerClient::new(reader, writer);

        let qualification = controller.qualify(b"q").expect("qualification");
        assert_eq!(qualification.launcher_peer, peer);
        assert_eq!(qualification.launcher_ready_frame, qualification_ready);
        let first = controller.execute(b"h1", b"r1").expect("first execution");
        assert_eq!(first.launcher_peer, peer);
        assert_eq!(first.launcher_ready_frame, execution_ready_one);
        assert_eq!(first.launcher_report_frame, execution_report_one);
        let second = controller.execute(b"h2", b"r2").expect("second execution");
        assert_eq!(second.launcher_peer, peer);
        assert_eq!(second.launcher_ready_frame, execution_ready_two);
        assert_eq!(second.launcher_report_frame, execution_report_two);
        controller.shutdown().expect("shutdown");

        let (_, written) = controller.into_inner().expect("sole controller owner");
        assert_eq!(super::decode_request_kinds(&written), vec![1, 2, 2, 3]);
    }

    #[test]
    fn controller_rejects_wrong_request_binding_or_launcher_peer() {
        let peer = ControllerLauncherPeer {
            pid: 4242,
            uid: 0,
            gid: 0,
        };
        let wrong_id = [0x55; 32];
        let input = encode_response_for_test(
            ControllerCommand::Execution,
            wrong_id,
            Some(peer),
            &[
                b"proxy".as_slice(),
                b"ready".as_slice(),
                b"report".as_slice(),
            ],
        );
        let controller = Mac4ControllerClient::new(Cursor::new(input), Vec::new());
        assert!(controller.execute(b"hello", b"request").is_err());

        let zero_peer = ControllerLauncherPeer {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let request_id = super::request_id(
            ControllerCommand::Execution,
            &[b"hello".as_slice(), b"request".as_slice()],
        );
        let input = encode_response_for_test(
            ControllerCommand::Execution,
            request_id,
            Some(zero_peer),
            &[
                b"proxy".as_slice(),
                b"ready".as_slice(),
                b"report".as_slice(),
            ],
        );
        let controller = Mac4ControllerClient::new(Cursor::new(input), Vec::new());
        assert!(controller.execute(b"hello", b"request").is_err());
    }

    #[test]
    fn controller_rejects_oversized_or_trailing_response_payloads() {
        let peer = ControllerLauncherPeer {
            pid: 4242,
            uid: 0,
            gid: 0,
        };
        let request_id = super::request_id(
            ControllerCommand::Execution,
            &[b"hello".as_slice(), b"request".as_slice()],
        );
        let mut response = encode_response_for_test(
            ControllerCommand::Execution,
            request_id,
            Some(peer),
            &[
                b"proxy".as_slice(),
                b"ready".as_slice(),
                b"report".as_slice(),
            ],
        );
        response.write_all(b"trailing").unwrap();
        let controller = Mac4ControllerClient::new(Cursor::new(response), Vec::new());
        controller
            .execute(b"hello", b"request")
            .expect("first exact response");
        assert!(
            controller.shutdown().is_err(),
            "trailing bytes cannot form a response"
        );

        let oversized = vec![0_u8; super::CONTROLLER_FRAME_CAP_BYTES + 1];
        assert!(
            super::encode_request_for_test(ControllerCommand::Execution, &[&oversized]).is_err()
        );
    }

    #[test]
    fn verified_controller_is_materialized_from_the_retained_handle_not_its_path() {
        let fixture = FixtureDirectory::new("materialize");
        let source_path = fixture.0.join("source-controller");
        let original = b"#!/bin/sh\nexit 0\n";
        fs::write(&source_path, original).expect("write source controller");
        let source = fs::File::open(&source_path).expect("open source controller");
        let replacement = fixture.0.join("replacement");
        fs::write(&replacement, b"tampered-after-open").expect("write replacement");
        fs::rename(&replacement, &source_path).expect("replace source path");
        let expected_digest = hex::encode(Sha256::digest(original));

        let materialized = super::materialize_verified_controller_file(
            &source,
            original.len() as u64,
            &expected_digest,
            &fixture.0,
        )
        .expect("materialize verified controller");

        assert_eq!(
            fs::read(materialized.path()).expect("read materialized"),
            original
        );
        assert_eq!(
            fs::metadata(materialized.path())
                .expect("materialized metadata")
                .permissions()
                .mode()
                & 0o777,
            0o500
        );
        let runtime_directory = materialized.runtime_directory().to_path_buf();
        drop(materialized);
        assert!(!runtime_directory.exists());
    }

    #[test]
    fn materialization_rejects_digest_drift_and_nonprivate_runtime_root() {
        let fixture = FixtureDirectory::new("materialize-reject");
        let source_path = fixture.0.join("source-controller");
        fs::write(&source_path, b"controller").expect("write source controller");
        let source = fs::File::open(&source_path).expect("open source controller");
        let error =
            super::materialize_verified_controller_file(&source, 10, &"00".repeat(32), &fixture.0)
                .expect_err("wrong digest is rejected");
        assert!(error.to_string().contains("digest"));
        assert!(!fixture.0.join("active-controller").exists());

        fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o755))
            .expect("make runtime root too broad");
        let error = super::materialize_verified_controller_file(
            &source,
            10,
            &hex::encode(Sha256::digest(b"controller")),
            &fixture.0,
        )
        .expect_err("nonprivate runtime root is rejected");
        assert!(error.to_string().contains("private"));
    }

    #[test]
    fn failed_controller_diagnostics_keep_only_the_bounded_console_tail() {
        let fixture = FixtureDirectory::new("failure-diagnostic-tail");
        let console = fixture.0.join("guest-console.log");
        fs::write(
            &console,
            b"prefix-that-must-be-cut\nlauncher-refused-policy\n",
        )
        .expect("write guest console");

        assert_eq!(
            super::read_bounded_runtime_diagnostic(&console, 25).expect("read bounded diagnostic"),
            "launcher-refused-policy\n"
        );
    }

    #[test]
    fn verified_boot_input_is_copied_from_its_retained_handle_as_read_only() {
        let fixture = FixtureDirectory::new("boot-input");
        let runtime = fixture.0.join("private-runtime");
        fs::create_dir(&runtime).expect("create private runtime");
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))
            .expect("protect private runtime");
        let source_path = fixture.0.join("source-kernel");
        let original = b"verified-kernel-bytes";
        fs::write(&source_path, original).expect("write source kernel");
        let source = fs::File::open(&source_path).expect("open retained kernel handle");
        let replacement = fixture.0.join("replacement-kernel");
        fs::write(&replacement, b"tampered-after-open").expect("write replacement kernel");
        fs::rename(&replacement, &source_path).expect("replace kernel path");

        let path = super::materialize_verified_runtime_file(
            &source,
            original.len() as u64,
            &hex::encode(Sha256::digest(original)),
            &runtime,
            "guest-kernel",
            0o400,
        )
        .expect("materialize retained kernel");
        assert_eq!(fs::read(&path).expect("read private kernel"), original);
        assert_eq!(
            fs::metadata(&path)
                .expect("private kernel metadata")
                .permissions()
                .mode()
                & 0o777,
            0o400
        );
    }

    #[test]
    fn boot_tuple_binding_is_derived_from_all_three_signed_image_digests() {
        let kernel = "11".repeat(32);
        let initrd = "22".repeat(32);
        let root_disk = "33".repeat(32);
        let mut expected = Sha256::new();
        expected.update(b"boole.mac4.boot-tuple.v1\0");
        expected.update([0x11; 32]);
        expected.update([0x22; 32]);
        expected.update([0x33; 32]);

        assert_eq!(
            super::boot_tuple_binding_hex(&kernel, &initrd, &root_disk)
                .expect("derive boot tuple binding"),
            hex::encode(expected.finalize())
        );
        assert!(super::boot_tuple_binding_hex(&kernel, &initrd, "not-a-digest").is_err());
    }

    #[test]
    fn direct_boot_binding_covers_only_the_two_files_supplied_to_the_vm() {
        let kernel = "11".repeat(32);
        let root_disk = "33".repeat(32);
        let mut expected = Sha256::new();
        expected.update(b"boole.mac4.boot-tuple.v2\0");
        expected.update([0x11; 32]);
        expected.update([0x33; 32]);

        assert_eq!(
            super::direct_boot_binding_hex(&kernel, &root_disk)
                .expect("derive direct boot binding"),
            hex::encode(expected.finalize())
        );
        assert!(super::direct_boot_binding_hex(&kernel, "not-a-digest").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn spawned_controller_rejects_dry_run_and_duplicate_stdio_modes() {
        for forbidden in [
            "--controller-stdio",
            "--controller-protocol-dry-run",
            "--dry-run",
            "--proxy-dry-run",
        ] {
            let error = super::require_production_controller_arguments(&[forbidden.into()])
                .expect_err("mode override is rejected");
            assert!(error.to_string().contains("forbidden mode override"));
        }
        super::require_production_controller_arguments(&[
            "--kernel".into(),
            "/private/runtime/kernel".into(),
        ])
        .expect("ordinary production arguments remain available");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bootable_controller_arguments_are_complete_and_not_caller_selected() {
        let arguments = super::bootable_controller_arguments(
            std::path::Path::new("/private/runtime/guest-kernel"),
            std::path::Path::new("/private/runtime/guest-root-disk"),
            std::path::Path::new("/private/runtime/guest-console.log"),
            std::path::Path::new("/private/runtime/guest-receipt.json"),
            &"11".repeat(32),
            &"22".repeat(32),
            &"33".repeat(32),
            &"44".repeat(32),
        )
        .expect("derive fixed production controller arguments");
        let strings = arguments
            .iter()
            .map(|value| value.to_str().expect("UTF-8 argument"))
            .collect::<Vec<_>>();
        assert_eq!(
            strings,
            vec![
                "--kernel",
                "/private/runtime/guest-kernel",
                "--kernel-sha256",
                &"11".repeat(32),
                "--root-disk",
                "/private/runtime/guest-root-disk",
                "--root-disk-sha256",
                &"22".repeat(32),
                "--cmdline",
                "console=hvc0 root=/dev/vda ro init=/usr/lib/systemd/systemd",
                "--nonce-hex",
                &"33".repeat(32),
                "--boot-binding-hex",
                &"44".repeat(32),
                "--console",
                "/private/runtime/guest-console.log",
                "--receipt",
                "/private/runtime/guest-receipt.json",
                "--timeout",
                "115",
            ]
        );
    }
}
