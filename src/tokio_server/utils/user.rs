use std::ffi::CString;
use std::io;

#[derive(Debug)]
pub struct UserPrivileges {
    uid: u32,
    gid: u32,
}

impl UserPrivileges {
    pub fn new(username: &str) -> io::Result<Self> {
        let c_username = CString::new(username)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        unsafe {
            let pw = libc::getpwnam(c_username.as_ptr());
            if pw.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Error: could not find user \"{}\"", username),
                ));
            }

            Ok(Self {
                uid: (*pw).pw_uid,
                gid: (*pw).pw_gid,
            })
        }
    }

    pub fn drop_privileges(&self) -> io::Result<()> {
        unsafe {
            if libc::setgid(self.gid) != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Error: failed to set group"
                ));
            }

            if libc::setuid(self.uid) != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Error: failed to set user"
                ));
            }

            Ok(())
        }
    }

    pub fn check_root() -> io::Result<()> {
        unsafe {
            if libc::getuid() != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Error: must be root to use option -u"
                ));
            }
            Ok(())
        }
    }
}