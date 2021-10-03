use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum DownloaderError {
    NotAuthorized,
    Authentication,
    APIException(String),
    APILatestArticle,
    APINameMissing,
    APIDateMissing,
}

impl fmt::Display for DownloaderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NotAuthorized => write!(f, "Not authorized, try updating cookies file"),
            Self::Authentication => write!(f, "Authentication error"),
            Self::APIException(s) => write!(f, "Daum API Error: {}", s),
            Self::APILatestArticle => write!(f, "Could not get latest post"),
            Self::APINameMissing => write!(f, "Missing field 'plainTextOfName'"),
            Self::APIDateMissing => write!(f, "Missing field 'regDttm'"),
        }
    }
}


impl Error for DownloaderError {}
