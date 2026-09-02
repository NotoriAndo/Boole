#![allow(unsafe_code)]

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::c_void;
    use std::io::{self, Read, Write};
    use std::mem;
    use std::os::fd::RawFd;
    use std::os::unix::net::UnixStream;
    use std::thread;

    use boole_native_shadow_mac4_relay::{
        decode_hello, encode_ready, serve_proxy_connection, GuestReady, FRAME_BYTES, HOST_CID,
        PROXY_VSOCK_PORT, VSOCK_PORT,
    };

    const AF_VSOCK: i32 = 40;
    const SOCK_STREAM: i32 = 1;
    const SOCK_CLOEXEC: i32 = 0o2_000_000;
    const SOL_SOCKET: i32 = 1;
    const SO_RCVTIMEO: i32 = 20;
    const SO_SNDTIMEO: i32 = 21;
    const SO_PEERCRED: i32 = 17;
    const VMADDR_CID_ANY: u32 = u32::MAX;
    const LAUNCHER_SOCKET_PATH: &str = "/run/boole/native-shadow/launcher.sock";

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SockaddrVm {
        family: u16,
        reserved1: u16,
        port: u32,
        cid: u32,
        flags: u8,
        zero: [u8; 3],
    }

    #[repr(C)]
    struct Timeval {
        seconds: i64,
        microseconds: i64,
    }

    #[repr(C)]
    struct Ucred {
        pid: i32,
        uid: u32,
        gid: u32,
    }

    unsafe extern "C" {
        fn socket(domain: i32, socket_type: i32, protocol: i32) -> i32;
        fn bind(socket: i32, address: *const c_void, address_len: u32) -> i32;
        fn listen(socket: i32, backlog: i32) -> i32;
        fn accept4(socket: i32, address: *mut c_void, address_len: *mut u32, flags: i32) -> i32;
        fn getpeername(socket: i32, address: *mut c_void, address_len: *mut u32) -> i32;
        fn setsockopt(
            socket: i32,
            level: i32,
            option: i32,
            value: *const c_void,
            value_len: u32,
        ) -> i32;
        fn getsockopt(
            socket: i32,
            level: i32,
            option: i32,
            value: *mut c_void,
            value_len: *mut u32,
        ) -> i32;
        fn read(socket: i32, buffer: *mut c_void, count: usize) -> isize;
        fn write(socket: i32, buffer: *const c_void, count: usize) -> isize;
        fn close(socket: i32) -> i32;
    }

    struct Descriptor(RawFd);

    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe {
                close(self.0);
            }
        }
    }

    fn last_error(context: &str) -> io::Error {
        let error = io::Error::last_os_error();
        io::Error::new(error.kind(), format!("{context}: {error}"))
    }

    fn vm_address(cid: u32, port: u32) -> SockaddrVm {
        SockaddrVm {
            family: AF_VSOCK as u16,
            reserved1: 0,
            port,
            cid,
            flags: 0,
            zero: [0; 3],
        }
    }

    fn listener(port: u32) -> io::Result<Descriptor> {
        let descriptor = unsafe { socket(AF_VSOCK, SOCK_STREAM | SOCK_CLOEXEC, 0) };
        if descriptor < 0 {
            return Err(last_error("create AF_VSOCK listener"));
        }
        let descriptor = Descriptor(descriptor);
        let address = vm_address(VMADDR_CID_ANY, port);
        let bound = unsafe {
            bind(
                descriptor.0,
                (&address as *const SockaddrVm).cast(),
                mem::size_of::<SockaddrVm>() as u32,
            )
        };
        if bound != 0 {
            return Err(last_error("bind AF_VSOCK listener"));
        }
        if unsafe { listen(descriptor.0, 1) } != 0 {
            return Err(last_error("listen on AF_VSOCK"));
        }
        Ok(descriptor)
    }

    fn accept_host(listener: &Descriptor) -> io::Result<Descriptor> {
        let accepted = unsafe {
            accept4(
                listener.0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                SOCK_CLOEXEC,
            )
        };
        if accepted < 0 {
            return Err(last_error("accept AF_VSOCK connection"));
        }
        let accepted = Descriptor(accepted);
        let mut peer = vm_address(0, 0);
        let mut length = mem::size_of::<SockaddrVm>() as u32;
        if unsafe {
            getpeername(
                accepted.0,
                (&mut peer as *mut SockaddrVm).cast(),
                &mut length,
            )
        } != 0
        {
            return Err(last_error("read AF_VSOCK peer identity"));
        }
        if length as usize != mem::size_of::<SockaddrVm>()
            || peer.family != AF_VSOCK as u16
            || peer.cid != HOST_CID
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "AF_VSOCK peer is not VMADDR_CID_HOST",
            ));
        }
        set_timeout(&accepted, SO_RCVTIMEO, 10)?;
        set_timeout(&accepted, SO_SNDTIMEO, 10)?;
        Ok(accepted)
    }

    fn set_timeout(descriptor: &Descriptor, option: i32, seconds: i64) -> io::Result<()> {
        let timeout = Timeval {
            seconds,
            microseconds: 0,
        };
        if unsafe {
            setsockopt(
                descriptor.0,
                SOL_SOCKET,
                option,
                (&timeout as *const Timeval).cast(),
                mem::size_of::<Timeval>() as u32,
            )
        } != 0
        {
            return Err(last_error("set AF_VSOCK connection timeout"));
        }
        Ok(())
    }

    impl Read for Descriptor {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            loop {
                let count = unsafe { read(self.0, buffer.as_mut_ptr().cast(), buffer.len()) };
                if count >= 0 {
                    return Ok(count as usize);
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }

    impl Write for Descriptor {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            loop {
                let count = unsafe { write(self.0, buffer.as_ptr().cast(), buffer.len()) };
                if count >= 0 {
                    return Ok(count as usize);
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn read_exact(descriptor: &Descriptor, mut output: &mut [u8]) -> io::Result<()> {
        while !output.is_empty() {
            let count = unsafe { read(descriptor.0, output.as_mut_ptr().cast(), output.len()) };
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "AF_VSOCK request ended before its fixed frame",
                ));
            }
            if count < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(io::Error::new(
                    error.kind(),
                    format!("read request: {error}"),
                ));
            }
            output = &mut output[count as usize..];
        }
        Ok(())
    }

    fn write_all(descriptor: &Descriptor, mut input: &[u8]) -> io::Result<()> {
        while !input.is_empty() {
            let count = unsafe { write(descriptor.0, input.as_ptr().cast(), input.len()) };
            if count <= 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(io::Error::new(
                    error.kind(),
                    format!("write response: {error}"),
                ));
            }
            input = &input[count as usize..];
        }
        Ok(())
    }

    fn serve_liveness_connection(connection: &Descriptor) -> io::Result<()> {
        let mut request = [0_u8; FRAME_BYTES];
        read_exact(connection, &mut request)?;
        let hello = decode_hello(&request)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let response = encode_ready(&GuestReady::for_hello(&hello));
        write_all(connection, &response)
    }

    fn launcher_peer(stream: &UnixStream) -> io::Result<(u32, u32, u32)> {
        use std::os::fd::AsRawFd;

        let mut credentials = Ucred {
            pid: 0,
            uid: u32::MAX,
            gid: u32::MAX,
        };
        let mut length = mem::size_of::<Ucred>() as u32;
        if unsafe {
            getsockopt(
                stream.as_raw_fd(),
                SOL_SOCKET,
                SO_PEERCRED,
                (&mut credentials as *mut Ucred).cast(),
                &mut length,
            )
        } != 0
        {
            return Err(last_error("read launcher SO_PEERCRED"));
        }
        if length as usize != mem::size_of::<Ucred>() || credentials.pid <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "launcher peer credentials are incomplete",
            ));
        }
        Ok((credentials.pid as u32, credentials.uid, credentials.gid))
    }

    fn serve_execution_proxy(connection: &mut Descriptor) -> io::Result<()> {
        set_timeout(connection, SO_RCVTIMEO, 120)?;
        set_timeout(connection, SO_SNDTIMEO, 120)?;
        let mut launcher = UnixStream::connect(LAUNCHER_SOCKET_PATH)
            .map_err(|error| io::Error::new(error.kind(), format!("connect launcher: {error}")))?;
        launcher.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
        launcher.set_write_timeout(Some(std::time::Duration::from_secs(120)))?;
        let (pid, uid, gid) = launcher_peer(&launcher)?;
        serve_proxy_connection(connection, &mut launcher, pid, uid, gid)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn run() -> io::Result<()> {
        let liveness_listener = listener(VSOCK_PORT)?;
        let proxy_listener = listener(PROXY_VSOCK_PORT)?;
        println!("BOOLE_MAC4_RELAY_READY port={VSOCK_PORT} hostCid={HOST_CID}");
        println!("BOOLE_MAC4_EXECUTION_PROXY_READY port={PROXY_VSOCK_PORT} hostCid={HOST_CID}");
        thread::spawn(move || loop {
            match accept_host(&proxy_listener)
                .and_then(|mut connection| serve_execution_proxy(&mut connection))
            {
                Ok(()) => println!("BOOLE_MAC4_EXECUTION_PROXY_COMPLETE"),
                Err(error) => eprintln!("boole-mac4-relay: refused proxy: {error}"),
            }
        });
        loop {
            match accept_host(&liveness_listener)
                .and_then(|connection| serve_liveness_connection(&connection))
            {
                Ok(()) => println!("BOOLE_MAC4_AUTHENTICATED_HANDSHAKE_COMPLETE"),
                Err(error) => eprintln!("boole-mac4-relay: refused connection: {error}"),
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux::run() {
        eprintln!("boole-mac4-relay: fatal: {error}");
        std::process::exit(2);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("boole-mac4-relay: AF_VSOCK relay requires Linux");
    std::process::exit(2);
}
