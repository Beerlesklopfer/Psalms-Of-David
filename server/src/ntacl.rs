// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hiding a folder on Windows with NTFS ACLs (project owner, 2026-10-28: "wir brauchen auch das
//! Verstecken unter Windows über ntacl"). The dot in `.quarantine` hides it on Linux; Windows has no
//! such rule. Here the folder gets a protected ACL that lets only SYSTEM, the Administrators and the
//! owner (the account the server runs as) in, so the family's other accounts neither see into it
//! nor change it, and the attributes "hidden" and "system" keep it out of Explorer's list.
//! On other systems `hide` does nothing.

use std::io;
use std::path::Path;

/// The ACL as SDDL: a protected DACL (nothing inherited) with full control for SYSTEM (`SY`),
/// the Administrators (`BA`) and the owner (`OW`), inherited by everything below the folder.
pub const SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;OW)";

#[cfg(not(windows))]
pub fn hide(_folder: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
pub fn hide(folder: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{FALSE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW};
    use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION};
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, SetFileAttributesW};

    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let path = wide(folder.as_os_str());
    let sddl = wide(std::ffi::OsStr::new(SDDL));
    // SAFETY: the strings are NUL-terminated and outlive the calls; the security descriptor made by the
    // conversion is freed with `LocalFree` once, and the DACL pointer is only used while it lives.
    unsafe {
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut descriptor, null_mut()) == 0 {
            return Err(io::Error::last_os_error());
        }
        let (mut present, mut defaulted, mut dacl) = (0, FALSE, null_mut());
        let mut result = if GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) == 0 { Err(io::Error::last_os_error()) } else { Ok(()) };
        if result.is_ok() {
            let code = SetNamedSecurityInfoW(path.as_ptr().cast_mut(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION, null_mut(), null_mut(), dacl, null());
            if code != 0 {
                result = Err(io::Error::from_raw_os_error(code as i32));
            }
        }
        LocalFree(descriptor);
        result?;
        if SetFileAttributesW(path.as_ptr(), FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_server_and_the_administrators_are_let_in() {
        assert_eq!(SDDL.matches("(A;OICI;FA;;;").count(), 3, "three full-control entries and no other");
        assert!(SDDL.starts_with("D:P"), "protected: nothing is inherited from the parent");
        assert!(!SDDL.contains(";;;WD") && !SDDL.contains(";;;BU"), "not Everyone, not the users");
    }

    #[cfg(not(windows))]
    #[test]
    fn it_does_nothing_where_a_dot_hides_the_folder() {
        assert!(hide(Path::new("/nonexistent/.quarantine")).is_ok());
    }
}
