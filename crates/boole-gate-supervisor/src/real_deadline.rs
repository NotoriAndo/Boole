//! Absolute preparation/execution deadlines and bounded post-run verification.
use anyhow::{ensure, Result};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};
#[derive(Debug)]
pub struct Expired;
impl std::fmt::Display for Expired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("real preparation/recheck absolute deadline")
    }
}
impl std::error::Error for Expired {}
#[derive(Clone, Copy)]
pub struct Deadline {
    expires: Instant,
    memory_mib: u64,
}
impl Deadline {
    pub fn after(ms: u64) -> Self {
        Self {
            expires: Instant::now() + Duration::from_millis(ms),
            memory_mib: crate::process::Envelope::default().memory_mib,
        }
    }
    pub fn for_envelope(envelope: &crate::process::Envelope) -> Self {
        Self {
            expires: Instant::now() + Duration::from_millis(envelope.wall_ms),
            memory_mib: envelope.memory_mib,
        }
    }
    pub fn memory_mib(self) -> u64 {
        self.memory_mib
    }
    pub fn check(self) -> Result<()> {
        if Instant::now() >= self.expires {
            return Err(Expired.into());
        }
        Ok(())
    }
    pub fn remaining(self) -> Result<u64> {
        self.check()?;
        Ok(self
            .expires
            .saturating_duration_since(Instant::now())
            .as_millis()
            .max(1)
            .min(u128::from(u64::MAX)) as u64)
    }
    pub fn hash(self, bytes: &[u8]) -> Result<boole_derivation::m2::Hash> {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        for chunk in bytes.chunks(65536) {
            self.check()?;
            hash.update(chunk);
        }
        self.check()?;
        Ok(hash.finalize().into())
    }
    pub fn read(self, r: &mut impl Read, b: &mut [u8]) -> Result<usize> {
        self.check()?;
        #[cfg(test)]
        {
            let delay = SLOW_READS.with(|v| {
                let mut v = v.borrow_mut();
                if let Some((after, delay, count)) = v.as_mut() {
                    *count += 1;
                    if *count > *after {
                        *delay
                    } else {
                        0
                    }
                } else {
                    0
                }
            });
            if delay > 0 {
                std::thread::sleep(Duration::from_millis(delay));
            }
        }
        let n = r.read(b)?;
        self.check()?;
        Ok(n)
    }
    pub fn read_bounded(self, p: &Path, cap: usize) -> Result<Vec<u8>> {
        self.check()?;
        let m = fs::symlink_metadata(p)?;
        ensure!(
            m.is_file() && !m.file_type().is_symlink() && m.len() <= cap as u64,
            "real file kind/size"
        );
        let mut f = File::open(p)?;
        let mut v = vec![];
        let mut b = [0; 65536];
        loop {
            let n = self.read(&mut f, &mut b)?;
            if n == 0 {
                break;
            }
            ensure!(v.len() + n <= cap, "real file grew");
            v.extend_from_slice(&b[..n]);
        }
        Ok(v)
    }
    pub fn write_file(self, p: &Path, raw: &[u8]) -> Result<()> {
        self.check()?;
        crate::codec::private_file(p, &[])?;
        let mut f = File::options().write(true).open(p)?;
        for b in raw.chunks(65536) {
            self.check()?;
            f.write_all(b)?;
            self.check()?;
        }
        f.sync_all()?;
        self.check()
    }
}

/// Outer interruption has policy-v2 transient accounting at every real phase.
pub fn is_transient(error: &anyhow::Error) -> bool {
    error.is::<Expired>() || error.is::<crate::real_process::Unavailable>()
}
#[cfg(test)]
thread_local! { static SLOW_READS: std::cell::RefCell<Option<(usize,u64,usize)>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(crate) fn slow_reads<T>(after: usize, delay_ms: u64, action: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            SLOW_READS.with(|v| *v.borrow_mut() = None);
        }
    }
    SLOW_READS.with(|v| {
        assert!(v.borrow().is_none());
        *v.borrow_mut() = Some((after, delay_ms, 0));
    });
    let reset = Reset;
    let result = action();
    let count = SLOW_READS.with(|v| v.borrow().as_ref().unwrap().2);
    drop(reset);
    (result, count)
}
