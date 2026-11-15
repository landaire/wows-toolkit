use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::ptr::null;
use std::ptr::null_mut;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_FIRST_PIPE_INSTANCE;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;

use wgc_relay_protocol::MAX_MESSAGE_LEN;
use wgc_relay_protocol::read_frame;
use wgc_relay_protocol::write_frame;
use windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE;
use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
use windows_sys::Win32::Foundation::ERROR_PIPE_CONNECTED;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::Storage::FileSystem::WriteFile;
use windows_sys::Win32::System::Pipes::ConnectNamedPipe;
use windows_sys::Win32::System::Pipes::CreateNamedPipeW;
use windows_sys::Win32::System::Pipes::PIPE_READMODE_MESSAGE;
use windows_sys::Win32::System::Pipes::PIPE_TYPE_MESSAGE;
use windows_sys::Win32::System::Pipes::PIPE_UNLIMITED_INSTANCES;
use windows_sys::Win32::System::Pipes::PIPE_WAIT;

use crate::error::RelayError;
use crate::link::LinkConfig;

/// One server end of `\\.\pipe\WGC_IPC`. `wgc_api.dll` opens a new client
/// connection per request, so each connected instance serves one client.
pub struct PipeInstance(OwnedHandle);

impl PipeInstance {
    /// The relay's first instance. Fails if another relay or a real WGC in
    /// this prefix already serves the pipe, which would split requests, and
    /// with them account tokens, between two launchers.
    pub fn create_first() -> Result<PipeInstance, RelayError> {
        PipeInstance::create_with(FILE_FLAG_FIRST_PIPE_INSTANCE)
    }

    fn create() -> Result<PipeInstance, RelayError> {
        PipeInstance::create_with(0)
    }

    fn create_with(extra_flags: FILE_FLAGS_AND_ATTRIBUTES) -> Result<PipeInstance, RelayError> {
        const BUFFER_LEN: u32 = 64 * 1024;

        let name: Vec<u16> = r"\\.\pipe\WGC_IPC".encode_utf16().chain(Some(0)).collect();
        // SAFETY: `name` is NUL-terminated; default security attributes.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | extra_flags,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER_LEN,
                BUFFER_LEN,
                0,
                null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(RelayError::last_os_error("CreateNamedPipeW"));
        }

        // SAFETY: a fresh handle this instance now owns.
        Ok(PipeInstance(unsafe { OwnedHandle::from_raw_handle(handle) }))
    }

    fn wait_for_client(&self) -> Result<(), RelayError> {
        // SAFETY: synchronous connect on an owned pipe handle.
        let connected = unsafe { ConnectNamedPipe(self.0.as_raw_handle(), null_mut()) } != 0;
        // SAFETY: reads thread-local state only.
        if connected || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED {
            return Ok(());
        }

        Err(RelayError::last_os_error("ConnectNamedPipe"))
    }

    /// Reads one whole message. `None` means the client closed the pipe.
    fn read_message(&self) -> Result<Option<Vec<u8>>, RelayError> {
        const CHUNK_LEN: usize = 16 * 1024;

        let mut message = Vec::new();
        loop {
            let start = message.len();
            message.resize(start + CHUNK_LEN, 0);
            let mut read = 0u32;
            // SAFETY: the buffer slice is live and CHUNK_LEN bytes long.
            let ok = unsafe {
                ReadFile(self.0.as_raw_handle(), message[start..].as_mut_ptr(), CHUNK_LEN as u32, &mut read, null_mut())
            } != 0;
            message.truncate(start + read as usize);
            if ok {
                return Ok(Some(message));
            }

            // SAFETY: reads thread-local state only.
            match unsafe { GetLastError() } {
                ERROR_MORE_DATA if message.len() < MAX_MESSAGE_LEN as usize => {}
                ERROR_BROKEN_PIPE => return Ok(None),
                _ => return Err(RelayError::last_os_error("ReadFile")),
            }
        }
    }

    fn write_message(&self, message: &[u8]) -> Result<(), RelayError> {
        let mut written = 0u32;
        // SAFETY: `message` is live for the call; message mode writes it whole.
        let ok = unsafe {
            WriteFile(self.0.as_raw_handle(), message.as_ptr(), message.len() as u32, &mut written, null_mut())
        } != 0;
        if !ok {
            return Err(RelayError::last_os_error("WriteFile"));
        }

        Ok(())
    }
}

/// Accepts clients forever, one thread per connection.
pub fn serve(first: PipeInstance, link: LinkConfig) {
    let mut instance = first;
    loop {
        let connected = instance.wait_for_client();

        // The client opens one connection per request; a listening instance
        // must exist before this one is handed off, or the next request finds
        // only busy instances.
        let next = match PipeInstance::create() {
            Ok(next) => next,
            Err(err) => {
                eprintln!("wgc-relay: {err}");
                return;
            }
        };
        let served = std::mem::replace(&mut instance, next);

        match connected {
            Ok(()) => {
                let link = link.clone();
                std::thread::spawn(move || {
                    if let Err(err) = relay_connection(&served, &link) {
                        eprintln!("wgc-relay: {err}");
                    }
                });
            }
            Err(err) => eprintln!("wgc-relay: {err}"),
        }
    }
}

fn relay_connection(pipe: &PipeInstance, link: &LinkConfig) -> Result<(), RelayError> {
    let mut launcher = None;
    while let Some(request) = pipe.read_message()? {
        let stream = match &mut launcher {
            Some(stream) => stream,
            None => launcher.insert(link.connect()?),
        };

        write_frame(stream, &request)?;
        let Some(reply) = read_frame(stream)? else {
            return Err(RelayError::Connect(std::io::ErrorKind::UnexpectedEof.into()));
        };
        pipe.write_message(&reply)?;
    }

    Ok(())
}
