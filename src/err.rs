use std::{fmt, io};

use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tracing::{error, warn};

#[derive(Debug)]
pub enum Error {
    Jinja(minijinja::Error),
    Io(io::Error),
    Sass(Box<grass::Error>),
    Axum(axum::Error),
    AxumHttp(axum::http::Error),
    AxumHttpHeader(axum::http::header::InvalidHeaderValue),

    ComponentRange(time::error::ComponentRange),
    Internal(color_eyre::Report),
}

impl std::error::Error for Error {}

#[cfg(feature = "development")]
impl fmt::Display for Error {
    #[allow(clippy::cognitive_complexity)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jinja(e) => {
                warn!(?e, "Jinja error");
                write!(f, "Jinja error: {e}")
            }
            Self::Io(e) => {
                warn!(?e, "IO Error");
                write!(f, "IO Error: {e}")
            }
            Self::Sass(e) => {
                warn!(?e, "Sass Error");
                write!(f, "Sass Error: {e}")
            }
            Self::Axum(e) => {
                warn!(?e, "Axum Error");
                write!(f, "Axum Error: {e}")
            }
            Self::AxumHttp(e) => {
                warn!(?e, "Axum HTTP Error");
                write!(f, "Axum HTTP Error: {e}")
            }
            Self::AxumHttpHeader(e) => {
                warn!(?e, "Axum HTTP Header Error");
                write!(f, "Axum HTTP Header Error: {e}")
            }
            Self::ComponentRange(e) => {
                warn!(?e, "Component Range Error");
                write!(f, "Component Range Error: {e}")
            }
            Self::Internal(e) => {
                warn!(?e, "Internal Error");
                write!(f, "Internal Error: {e}")
            }
        }
    }
}

#[cfg(feature = "production")]
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Jinja(e) => {
                warn!(?e, "Jinja error");
            }
            Self::Io(e) => {
                warn!(?e, "IO Error");
            }
            Self::Sass(e) => {
                warn!(?e, "Sass Error")
            }
            Self::Axum(e) => {
                warn!(?e, "Axum Error")
            }
            Self::AxumHttp(e) => {
                warn!(?e, "Axum HTTP Error")
            }
            Self::AxumHttpHeader(e) => {
                warn!(?e, "Axum HTTP Header Error");
            }
            Self::ComponentRange(e) => {
                warn!(?e, "Component Range Error")
            }
            Self::Internal(e) => {
                warn!(?e, "Internal Error")
            }
        }

        write!(f, "Internal Server Error")
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        match Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(self.to_string()))
        {
            Ok(resp) => resp,
            Err(e) => {
                error!(?e, "Failed to generate response from error.");

                // This is truly the worst place to fail. But it should *never* fail here.
                unreachable!("Should never ever fail here.");
            }
        }
    }
}

impl From<minijinja::Error> for Error {
    fn from(value: minijinja::Error) -> Self {
        Self::Jinja(value)
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<Box<grass::Error>> for Error {
    fn from(value: Box<grass::Error>) -> Self {
        Self::Sass(value)
    }
}

impl From<axum::Error> for Error {
    fn from(value: axum::Error) -> Self {
        Self::Axum(value)
    }
}

impl From<axum::http::Error> for Error {
    fn from(value: axum::http::Error) -> Self {
        Self::AxumHttp(value)
    }
}

impl From<axum::http::header::InvalidHeaderValue> for Error {
    fn from(value: axum::http::header::InvalidHeaderValue) -> Self {
        Self::AxumHttpHeader(value)
    }
}

impl From<time::error::ComponentRange> for Error {
    fn from(value: time::error::ComponentRange) -> Self {
        Self::ComponentRange(value)
    }
}

impl From<color_eyre::Report> for Error {
    fn from(value: color_eyre::Report) -> Self {
        Self::Internal(value)
    }
}
