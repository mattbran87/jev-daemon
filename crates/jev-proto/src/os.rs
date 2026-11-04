//! Windows calls: the current user's SID, and the owner of a kernel object
//! such as a named pipe.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};

use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TokenUser, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    PSID, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// The `TOKEN_USER` of this process. `u64` keeps the buffer aligned.
struct CurrentUser(Vec<u64>);

impl CurrentUser {
    fn get() -> io::Result<CurrentUser> {
        // SAFETY: each call gets valid pointers, and the token handle is
        // closed when `token` drops.
        unsafe {
            let mut raw: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = OwnedHandle::from_raw_handle(raw as RawHandle);
            let mut len = 0u32;
            GetTokenInformation(
                token.as_raw_handle() as HANDLE,
                TokenUser,
                std::ptr::null_mut(),
                0,
                &mut len,
            );
            if len == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
            if GetTokenInformation(
                token.as_raw_handle() as HANDLE,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                len,
                &mut len,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(CurrentUser(buffer))
        }
    }

    fn sid(&self) -> PSID {
        // SAFETY: the buffer holds a TOKEN_USER that GetTokenInformation wrote.
        unsafe { (*(self.0.as_ptr() as *const TOKEN_USER)).User.Sid }
    }
}

/// The current user's SID as text, such as `S-1-5-21-...-1001`.
pub fn user_sid() -> io::Result<String> {
    let user = CurrentUser::get()?;
    // SAFETY: `text` gets a LocalAlloc string that is freed below.
    unsafe {
        let mut text: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.sid(), &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Ok(sid)
    }
}

/// True when the current user owns the kernel object behind `handle`.
pub fn owned_by_current_user(handle: RawHandle) -> io::Result<bool> {
    let user = CurrentUser::get()?;
    // SAFETY: `owner` points into `descriptor`, which is freed after the compare.
    unsafe {
        let mut owner: PSID = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let status = GetSecurityInfo(
            handle as HANDLE,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        );
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let same = EqualSid(owner, user.sid()) != 0;
        LocalFree(descriptor.cast());
        Ok(same)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sid_has_the_text_form() {
        let sid = user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        assert!(sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S'));
    }

    #[test]
    fn this_user_owns_its_own_test_binary() {
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        assert!(owned_by_current_user(file.as_raw_handle()).unwrap());
    }

    #[test]
    fn this_user_does_not_own_a_system_file() {
        // TrustedInstaller owns cmd.exe.
        let file = std::fs::File::open(r"C:\Windows\System32\cmd.exe").unwrap();
        assert!(!owned_by_current_user(file.as_raw_handle()).unwrap());
    }
}
