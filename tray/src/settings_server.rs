// Serves the real settings.toml to every IME process.
//
// Start and Search virtualize the registry and %LOCALAPPDATA%, so the DLL
// inside them cannot open the user's file. This process is not packaged.
// The pipe name matches `SETTINGS_PIPE` in rcantonese/src/config.rs.
// Clients only need to read, and packaged hosts are not the interactive user
// in the ACL sense: Everyone plus both app-package SIDs can connect.

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE, HLOCAL, WIN32_ERROR};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{WriteFile, PIPE_ACCESS_OUTBOUND};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES};

const ERROR_PIPE_CONNECTED: u32 = 535;

pub fn start() {
        std::thread::spawn(|| {
                let mut reported = false;
                loop {
                        match listen() {
                                Ok(pipe) => {
                                        reported = false;
                                        serve(pipe);
                                }
                                Err(()) => {
                                        if !reported {
                                                crate::log_error("settings pipe failed to listen");
                                                reported = true;
                                        }
                                        std::thread::sleep(std::time::Duration::from_millis(500));
                                }
                        }
                }
        });
}

fn serve(pipe: HANDLE) {
        let connected = unsafe { ConnectNamedPipe(pipe, None).is_ok() || GetLastError() == WIN32_ERROR(ERROR_PIPE_CONNECTED) };
        if connected {
                if let Some(bytes) = read_user_settings() {
                        let mut off = 0;
                        while off < bytes.len() {
                                let mut wrote = 0u32;
                                let ok = unsafe { WriteFile(pipe, Some(&bytes[off..]), Some(&mut wrote), None).is_ok() };
                                if !ok || wrote == 0 {
                                        break;
                                }
                                off += wrote as usize;
                        }
                }
        }
        unsafe {
                let _ = DisconnectNamedPipe(pipe);
                let _ = CloseHandle(pipe);
        }
}

fn listen() -> Result<HANDLE, ()> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let sddl = w!("D:(A;;GA;;;WD)(A;;GA;;;AC)(A;;GA;;;S-1-15-2-2)");
        let made = unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl, 1, &mut sd, None).is_ok() && !sd.0.is_null() };
        let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: windows::core::BOOL(0),
        };
        let handle = unsafe {
                CreateNamedPipeW(
                        w!("\\\\.\\pipe\\RCantonese.Settings"),
                        PIPE_ACCESS_OUTBOUND,
                        PIPE_TYPE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                        PIPE_UNLIMITED_INSTANCES,
                        64 * 1024,
                        4096,
                        0,
                        made.then_some(&sa as *const SECURITY_ATTRIBUTES),
                )
        };
        if made {
                unsafe {
                        let _ = LocalFree(Some(HLOCAL(sd.0)));
                }
        }
        if handle.is_invalid() { Err(()) } else { Ok(handle) }
}

fn read_user_settings() -> Option<Vec<u8>> {
        let local = std::env::var_os("LOCALAPPDATA")?;
        let path = std::path::PathBuf::from(local).join("RCantonese").join("settings.toml");
        let bytes = std::fs::read(path).ok()?;
        if bytes.is_empty() { None } else { Some(bytes) }
}
