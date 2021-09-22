use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum DownloaderError {
    NotAuthenticatedException,
    AuthenticationError,
    APIException(String),
    APINameMissing,
    APIDateMissing,
}

impl fmt::Display for DownloaderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NotAuthenticatedException => write!(f, "Not authorized, try updating cookies file"),
            Self::AuthenticationError => write!(f, "Authentication error"),
            Self::APIException(s) => write!(f, "Daum API Error: {}", s),
            Self::APINameMissing => write!(f, "Missing field 'plainTextOfName'"),
            Self::APIDateMissing => write!(f, "Missing field 'regDttm'"),
        }
    }
}


impl Error for DownloaderError {}
