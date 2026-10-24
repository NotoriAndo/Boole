use std::fmt;

#[derive(Debug)]
pub enum Error {
    Startup {
        number: u8,
        detail: String,
    },
    Integrity {
        height: u64,
        class: &'static str,
        detail: String,
    },
    Availability {
        height: u64,
        class: &'static str,
        detail: String,
    },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Startup { number, detail } => write!(f, "startup S{number}: {detail}"),
            Self::Integrity {
                height,
                class,
                detail,
            } => write!(f, "M4 integrity at {height} ({class}): {detail}"),
            Self::Availability {
                height,
                class,
                detail,
            } => write!(f, "M4 availability at {height} ({class}): {detail}"),
        }
    }
}
impl std::error::Error for Error {}
pub fn integrity(height: u64, class: &'static str, detail: impl Into<String>) -> anyhow::Error {
    Error::Integrity {
        height,
        class,
        detail: detail.into(),
    }
    .into()
}
pub fn availability(height: u64, class: &'static str, detail: impl Into<String>) -> anyhow::Error {
    Error::Availability {
        height,
        class,
        detail: detail.into(),
    }
    .into()
}
pub fn startup(number: u8, detail: impl Into<String>) -> anyhow::Error {
    Error::Startup {
        number,
        detail: detail.into(),
    }
    .into()
}
