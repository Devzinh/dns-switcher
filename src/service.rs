use crate::sys;
use dns_switcher::ipc::{self, Request};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::io::FromRawHandle;
use std::time::Duration;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

define_windows_service!(ffi_service_main, service_main);

static STATUS: std::sync::OnceLock<service_control_handler::ServiceStatusHandle> = std::sync::OnceLock::new();

fn report(state: ServiceState, accept: ServiceControlAccept) {
    if let Some(status) = STATUS.get() {
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });
    }
}

pub fn run() -> Result<(), String> {
    service_dispatcher::start(ipc::SERVICE_NAME, ffi_service_main).map_err(|e| {
        format!("{e}. This mode is started by Windows; install DNS Switcher with its setup program.")
    })
}

fn service_main(_args: Vec<OsString>) {
    let handler = |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            // The pipe loop blocks in ConnectNamedPipe, so stop by exiting after reporting Stopped.
            std::thread::spawn(|| {
                report(ServiceState::Stopped, ServiceControlAccept::empty());
                std::process::exit(0);
            });
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let Ok(status) = service_control_handler::register(ipc::SERVICE_NAME, handler) else { return };
    let _ = STATUS.set(status);
    report(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN);
    crate::updater::start_background();
    serve_forever();
    report(ServiceState::Stopped, ServiceControlAccept::empty());
}

fn serve_forever() {
    loop {
        match accept_client() {
            Ok(pipe) => handle(pipe),
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
}

fn handle(pipe: std::fs::File) {
    let mut reader = BufReader::new(pipe.try_clone().expect("pipe handle"));
    let mut line = String::new();
    let read = (&mut reader).take(ipc::MAX_LINE as u64 + 2).read_line(&mut line);
    let reply = match read {
        Ok(_) => match Request::decode(&line) {
            Ok(Request::UpdateStatus) => Ok(crate::updater::status()),
            Ok(Request::UpdateNow) => crate::updater::check_and_install(),
            Ok(Request::AutoUpdate(on)) => crate::updater::set_auto(on),
            Ok(req) => sys::execute(&req),
            Err(e) => Err(format!("rejected: {e}")),
        },
        Err(e) => Err(e.to_string()),
    };
    let mut out = pipe;
    let _ = writeln!(out, "{}", ipc::encode_reply(&reply));
    let _ = out.flush();
}

fn accept_client() -> Result<std::fs::File, String> {
    use windows_sys::Win32::Foundation::{GetLastError, LocalFree, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    // SYSTEM and Administrators: full access; signed-in users: read/write.
    let sddl: Vec<u16> = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)\0".encode_utf16().collect();
    let name: Vec<u16> = format!("{}\0", ipc::PIPE_NAME).encode_utf16().collect();
    unsafe {
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut()) == 0 {
            return Err(format!("security descriptor: error {}", GetLastError()));
        }
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        let h = CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            &sa,
        );
        LocalFree(sd as _);
        if h == INVALID_HANDLE_VALUE {
            return Err(format!("CreateNamedPipe: error {}", GetLastError()));
        }
        let file = std::fs::File::from_raw_handle(h as _);
        if ConnectNamedPipe(h, std::ptr::null_mut()) == 0 && GetLastError() != ERROR_PIPE_CONNECTED {
            return Err(format!("ConnectNamedPipe: error {}", GetLastError()));
        }
        Ok(file)
    }
}

pub fn call(req: &Request) -> Result<String, Option<String>> {
    let pipe = std::fs::OpenOptions::new().read(true).write(true).open(ipc::PIPE_NAME);
    let mut pipe = match pipe {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(None),
        Err(e) => return Err(Some(format!("Could not reach the DNS Switcher service: {e}"))),
    };
    writeln!(pipe, "{}", req.encode()).map_err(|e| Some(e.to_string()))?;
    let mut line = String::new();
    BufReader::new(pipe).read_line(&mut line).map_err(|e| Some(e.to_string()))?;
    ipc::decode_reply(&line).map_err(Some)
}

pub fn perform(req: &Request) -> Result<String, String> {
    match call(req) {
        Ok(msg) => Ok(msg),
        Err(Some(e)) => Err(e),
        Err(None) if sys::is_elevated() => sys::execute(req),
        Err(None) => Err(
            "The DNS Switcher service isn't running. Install DNS Switcher with the setup program, or run this app as administrator."
                .into(),
        ),
    }
}
