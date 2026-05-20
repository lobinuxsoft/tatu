//! Module enumeration in the target process — `EnumProcessModulesEx` +
//! `GetModuleBaseNameW` + `GetModuleInformation` wrapper. The AOB
//! scanner uses [`find_module`] to bound a scan to one DLL/EXE; the
//! code patcher uses it to translate a `"game.exe+1A2B"` symbol from a
//! `.CT` table to a remote address.

use std::mem;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::ProcessStatus::{
    EnumProcessModulesEx, GetModuleBaseNameW, GetModuleInformation, LIST_MODULES_ALL, MODULEINFO,
};

#[derive(Debug, Clone)]
pub struct RemoteModule {
    pub name: String,
    pub base: u64,
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ModulesError {
    #[error("EnumProcessModulesEx returned 0 (os error {0})")]
    Enum(i32),
    #[error("GetModuleInformation returned 0 (os error {0})")]
    Info(i32),
}

const MAX_MODULES: usize = 1024;

pub fn list_modules(process: HANDLE) -> Result<Vec<RemoteModule>, ModulesError> {
    let mut handles = vec![0isize; MAX_MODULES];
    let cb_needed_in = (handles.len() * mem::size_of::<isize>()) as u32;
    let mut cb_needed: u32 = 0;

    let ok = unsafe {
        EnumProcessModulesEx(
            process,
            handles.as_mut_ptr() as *mut _,
            cb_needed_in,
            &mut cb_needed,
            LIST_MODULES_ALL,
        )
    };
    if ok == 0 {
        return Err(ModulesError::Enum(
            std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        ));
    }
    let count = (cb_needed as usize / mem::size_of::<isize>()).min(handles.len());

    let mut out = Vec::with_capacity(count);
    for &h in handles.iter().take(count) {
        let module: HANDLE = h as HANDLE;
        let name = base_name(process, module);

        let mut info: MODULEINFO = unsafe { mem::zeroed() };
        let ok = unsafe {
            GetModuleInformation(
                process,
                module,
                &mut info,
                mem::size_of::<MODULEINFO>() as u32,
            )
        };
        if ok == 0 {
            return Err(ModulesError::Info(
                std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
            ));
        }
        out.push(RemoteModule {
            name,
            base: info.lpBaseOfDll as u64,
            size: info.SizeOfImage as u64,
        });
    }
    Ok(out)
}

/// Case-insensitive search by module file name (e.g. `"Game.exe"`,
/// `"kernel32.dll"`).
pub fn find_module(process: HANDLE, name: &str) -> Result<Option<RemoteModule>, ModulesError> {
    let modules = list_modules(process)?;
    Ok(modules
        .into_iter()
        .find(|m| m.name.eq_ignore_ascii_case(name)))
}

fn base_name(process: HANDLE, module: HANDLE) -> String {
    let mut buf = [0u16; 260];
    let len = unsafe {
        GetModuleBaseNameW(
            process,
            module,
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    };
    if len == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

