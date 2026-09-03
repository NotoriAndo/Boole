use std::collections::HashMap;
use std::io;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, RwLockReadGuard, Weak};
use std::time::Duration;

/// One shutdown boundary for every blocking P2P worker and socket owned by a
/// node. Registering a clone does not change socket semantics; it only gives
/// shutdown a handle that can wake a blocked read/write before worker joins.
pub(crate) struct P2pLifecycle {
    stopped: AtomicBool,
    next_socket_id: AtomicU64,
    sockets: Mutex<HashMap<u64, TcpStream>>,
    mutation_gate: RwLock<()>,
    wake: Condvar,
}

impl P2pLifecycle {
    pub(crate) fn new() -> Self {
        Self {
            stopped: AtomicBool::new(false),
            next_socket_id: AtomicU64::new(1),
            sockets: Mutex::new(HashMap::new()),
            mutation_gate: RwLock::new(()),
            wake: Condvar::new(),
        }
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    pub(crate) fn register(self: &Arc<Self>, stream: &TcpStream) -> io::Result<SocketLease> {
        let shutdown_handle = stream.try_clone()?;
        let mut sockets = self.sockets.lock().expect("P2P socket registry poisoned");
        if self.is_stopped() {
            let _ = shutdown_handle.shutdown(Shutdown::Both);
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "P2P lifecycle is stopped",
            ));
        }
        let id = self.next_socket_id.fetch_add(1, Ordering::Relaxed);
        sockets.insert(id, shutdown_handle);
        Ok(SocketLease {
            id,
            lifecycle: Arc::downgrade(self),
        })
    }

    /// Returns true when shutdown won the wait, false when the duration
    /// elapsed normally.
    pub(crate) fn wait_or_stop(&self, duration: Duration) -> bool {
        if self.is_stopped() {
            return true;
        }
        let sockets = self.sockets.lock().expect("P2P socket registry poisoned");
        let (_sockets, _timeout) = self
            .wake
            .wait_timeout_while(sockets, duration, |_| !self.is_stopped())
            .expect("P2P lifecycle wait poisoned");
        self.is_stopped()
    }

    /// Linearization gate for mutations reached from network input. A permit
    /// acquired before `stop` may finish; once `stop` begins, no new permit is
    /// returned, and `stop` does not return until every earlier permit drops.
    pub(crate) fn begin_mutation(&self) -> Option<P2pMutationPermit<'_>> {
        let guard = self
            .mutation_gate
            .read()
            .expect("P2P mutation gate poisoned");
        if self.is_stopped() {
            None
        } else {
            Some(P2pMutationPermit { _guard: guard })
        }
    }

    /// Publish shutdown and wake every blocking P2P socket/wait immediately.
    /// This half never waits for an in-flight state mutation, so an HTTP
    /// graceful-drain trigger can close the network boundary before it waits
    /// for either HTTP or P2P work to finish.
    pub(crate) fn request_stop(&self) {
        self.stopped.store(true, Ordering::Release);
        {
            let sockets = self.sockets.lock().expect("P2P socket registry poisoned");
            for stream in sockets.values() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        self.wake.notify_all();
    }

    /// Close the lifecycle and cross the state-mutation barrier. Once this
    /// returns every earlier network-owned mutation has finished and no later
    /// one can begin.
    pub(crate) fn stop(&self) {
        self.request_stop();
        let _mutation_barrier = self
            .mutation_gate
            .write()
            .expect("P2P mutation gate poisoned");
    }

    #[cfg(test)]
    fn active_socket_count(&self) -> usize {
        self.sockets
            .lock()
            .expect("P2P socket registry poisoned")
            .len()
    }
}

pub(crate) struct P2pMutationPermit<'a> {
    _guard: RwLockReadGuard<'a, ()>,
}

pub(crate) struct SocketLease {
    id: u64,
    lifecycle: Weak<P2pLifecycle>,
}

impl Drop for SocketLease {
    fn drop(&mut self) {
        if let Some(lifecycle) = self.lifecycle.upgrade() {
            lifecycle
                .sockets
                .lock()
                .expect("P2P socket registry poisoned")
                .remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    fn connected_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let client = TcpStream::connect(address).expect("connect");
        let (server, _) = listener.accept().expect("accept");
        (client, server)
    }

    #[test]
    fn stop_closes_every_registered_socket_and_rejects_late_registration() {
        let lifecycle = Arc::new(P2pLifecycle::new());
        let (mut peer_a, socket_a) = connected_pair();
        let (mut peer_b, socket_b) = connected_pair();
        peer_a
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("timeout a");
        peer_b
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("timeout b");

        let _lease_a = lifecycle.register(&socket_a).expect("register a");
        let _lease_b = lifecycle.register(&socket_b).expect("register b");
        assert_eq!(lifecycle.active_socket_count(), 2);

        lifecycle.stop();

        let mut byte = [0_u8; 1];
        assert_eq!(peer_a.read(&mut byte).expect("peer a wakes"), 0);
        assert_eq!(peer_b.read(&mut byte).expect("peer b wakes"), 0);
        assert!(lifecycle.is_stopped());
        let (_late_peer, late_socket) = connected_pair();
        assert!(lifecycle.register(&late_socket).is_err());
    }

    #[test]
    fn stop_wakes_a_lifecycle_wait_without_polling_the_full_duration() {
        let lifecycle = Arc::new(P2pLifecycle::new());
        let waiter = lifecycle.clone();
        let started = Instant::now();
        let handle = thread::spawn(move || waiter.wait_or_stop(Duration::from_secs(5)));
        thread::sleep(Duration::from_millis(30));
        lifecycle.stop();
        assert!(handle.join().expect("waiter joins"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn dropping_a_lease_removes_only_its_socket() {
        let lifecycle = Arc::new(P2pLifecycle::new());
        let (_peer_a, socket_a) = connected_pair();
        let (_peer_b, socket_b) = connected_pair();
        let lease_a = lifecycle.register(&socket_a).expect("register a");
        let _lease_b = lifecycle.register(&socket_b).expect("register b");
        assert_eq!(lifecycle.active_socket_count(), 2);
        drop(lease_a);
        assert_eq!(lifecycle.active_socket_count(), 1);
        let _ = socket_a.shutdown(Shutdown::Both);
    }

    #[test]
    fn stop_waits_for_an_earlier_mutation_and_rejects_every_later_one() {
        let lifecycle = Arc::new(P2pLifecycle::new());
        let permit = lifecycle
            .begin_mutation()
            .expect("mutation starts before stop");
        let stopper_lifecycle = lifecycle.clone();
        let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();
        let stopper = thread::spawn(move || {
            stopper_lifecycle.stop();
            stopped_tx.send(()).expect("report stopped");
        });

        assert!(stopped_rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(permit);
        stopped_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("stop crosses the mutation barrier");
        stopper.join().expect("stopper joins");
        assert!(lifecycle.begin_mutation().is_none());
    }

    #[test]
    fn request_stop_wakes_sockets_before_an_earlier_mutation_finishes() {
        let lifecycle = Arc::new(P2pLifecycle::new());
        let permit = lifecycle
            .begin_mutation()
            .expect("mutation starts before stop request");
        let (mut peer, socket) = connected_pair();
        peer.set_read_timeout(Some(Duration::from_secs(1)))
            .expect("peer timeout");
        let _lease = lifecycle.register(&socket).expect("register socket");

        lifecycle.request_stop();

        let mut byte = [0_u8; 1];
        assert_eq!(peer.read(&mut byte).expect("peer wakes"), 0);
        assert!(lifecycle.is_stopped());
        drop(permit);
        lifecycle.stop();
    }
}
