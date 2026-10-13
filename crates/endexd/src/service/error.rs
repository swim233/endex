//! Error replies of `org.endex.Daemon1`.

use endex_core::search::Cancelled;
use zbus::message::{Header, Message};
use zbus::names::ErrorName;

use crate::bus;

/// Error replies, named `org.endex.Daemon1.Error.<variant>`.
#[derive(Debug)]
pub enum Error {
    /// A newer query from the same client superseded this one.
    Cancelled(String),
    InvalidArgs(String),
    NotFound(String),
    Failed(String),
    /// The daemon serves an index in read-only mode (`--data-dir`) and changes nothing.
    ReadOnly(String),
}

impl zbus::DBusError for Error {
    fn name(&self) -> ErrorName<'_> {
        ErrorName::from_static_str_unchecked(match self {
            Error::Cancelled(_) => "org.endex.Daemon1.Error.Cancelled",
            Error::InvalidArgs(_) => "org.endex.Daemon1.Error.InvalidArgs",
            Error::NotFound(_) => "org.endex.Daemon1.Error.NotFound",
            Error::Failed(_) => "org.endex.Daemon1.Error.Failed",
            Error::ReadOnly(_) => "org.endex.Daemon1.Error.ReadOnly",
        })
    }

    fn description(&self) -> Option<&str> {
        match self {
            Error::Cancelled(text)
            | Error::InvalidArgs(text)
            | Error::NotFound(text)
            | Error::Failed(text)
            | Error::ReadOnly(text) => Some(text),
        }
    }

    /// Error text can quote anything (a file, the config), so it goes through the guard too.
    fn create_reply(&self, call: &Header<'_>) -> zbus::Result<Message> {
        let text = bus::text(self.description().unwrap_or_default().to_owned());
        Message::error(call, self.name())?.build(&text)
    }
}

impl From<anyhow::Error> for Error {
    fn from(error: anyhow::Error) -> Self {
        if error.downcast_ref::<Cancelled>().is_some() {
            Error::Cancelled(error.to_string())
        } else {
            Error::Failed(format!("{error:#}"))
        }
    }
}
