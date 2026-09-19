//! Minimal `std` surface for the Switch port (`use siglus_switch_compat as std;`).
//! Grows module by module as engine crates are ported.
#![no_std]
extern crate alloc;

pub mod io {
    use alloc::string::String;
    use core::fmt;

    #[derive(Debug)]
    pub struct Error(String);

    impl Error {
        pub fn new(msg: String) -> Self {
            Self(msg)
        }
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl core::error::Error for Error {}

    pub type Result<T, E = Error> = core::result::Result<T, E>;
}

pub mod path {
    use alloc::string::String;
    use core::fmt;

    #[derive(Debug)]
    #[repr(transparent)]
    pub struct Path(str);

    impl Path {
        pub fn new(s: &str) -> &Path {
            unsafe { &*(s as *const str as *const Path) }
        }
        pub fn as_str(&self) -> &str {
            &self.0
        }
        pub fn display(&self) -> Display<'_> {
            Display(&self.0)
        }
    }

    pub struct Display<'a>(&'a str);

    impl fmt::Display for Display<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    #[derive(Debug, Clone)]
    pub struct PathBuf(String);

    impl PathBuf {
        pub fn new(s: impl Into<String>) -> Self {
            Self(s.into())
        }
    }

    impl core::ops::Deref for PathBuf {
        type Target = Path;
        fn deref(&self) -> &Path {
            Path::new(&self.0)
        }
    }
}

pub mod fs {
    use crate::io::Error;
    use crate::path::Path;
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use nx::fs::FileOpenOption;

    fn ioerr(e: impl core::fmt::Debug) -> Error {
        Error::new(format!("{e:?}"))
    }

    pub fn read(path: &Path) -> Result<Vec<u8>, Error> {
        let mut f = nx::fs::open_file(path.as_str(), FileOpenOption::Read()).map_err(ioerr)?;
        let size = f.get_size().map_err(ioerr)?;
        let mut buf = vec![0u8; size];
        let mut off = 0;
        while off < size {
            let n = f.read_array(&mut buf[off..]).map_err(ioerr)?;
            if n == 0 {
                break;
            }
            off += n;
        }
        buf.truncate(off);
        Ok(buf)
    }

    pub fn read_to_string(path: &Path) -> Result<String, Error> {
        let bytes = read(path)?;
        String::from_utf8(bytes).map_err(|e| Error::new(format!("utf8: {e}")))
    }
}

pub use core::str;
