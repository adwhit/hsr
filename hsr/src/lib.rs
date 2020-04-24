//! HSR runtime helpers and types

#[macro_use]
#[allow(unused_imports)]
extern crate serde_derive;
pub use serde_derive::{Deserialize, Serialize};

// We have a tonne of public imports. We places them here and make them public
// so that the user doesn't have to faff around adding them all and making sure
// the versions are all compatible
pub use actix_http;
pub use actix_rt;
pub use actix_web;
pub use async_trait;
pub use awc;
pub use futures;
pub use serde_json;
pub use serde_urlencoded;
pub use url;

pub use openssl;

pub use url::Url;

// We re-export this type as it is used in all the trait functions
use actix_http::http::StatusCode;
use actix_web::{Error as ActixError, HttpRequest, HttpResponse, Responder};
use std::fmt;

#[derive(Debug, Copy, Clone)]
/// Actix-web responder that always returns Ok
// replacement for returning bare '()' which was removed in 2.0
pub struct Success;

impl fmt::Display for Success {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "success")
    }
}

impl Responder for Success {
    type Error = actix_web::Error;
    type Future = futures::future::Ready<Result<HttpResponse, Self::Error>>;

    fn respond_to(self, req: &HttpRequest) -> Self::Future {
        return HttpResponse::Ok().finish().respond_to(req);
    }
}

/// Associate an http status code with a type. Defaults to 501 Internal Server Error
pub trait HasStatusCode {
    /// The http status code associated with the type
    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// Errors that may be returned by the client, apart from those explicitly
/// specified in the spec.
///
/// This will handle bad connections, path errors, unreconginized statuses
/// and any other 'unexpected errors'
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("Unknown status code: {:?}", _0)]
    BadStatus(StatusCode),
    #[error("Actix error: {}", _0)]
    Actix(#[from] ActixError),
}

impl HasStatusCode for ClientError {}

pub fn configure_spec(
    cfg: &mut actix_web::web::ServiceConfig,
    spec: &'static str,
    ui: &'static str,
) {
    use actix_web::http::header::ContentType;
    // Add route serving up the json spec
    cfg.route(
        "/spec.json",
        actix_web::web::get().to(move || HttpResponse::Ok().set(ContentType::json()).body(spec)),
    )
    // Add route serving up the rendered ui
    .route(
        "/ui.html",
        actix_web::web::get().to(move || HttpResponse::Ok().set(ContentType::html()).body(ui)),
    );
}

pub struct Config {
    pub host: Url,
    pub ssl: Option<openssl::ssl::SslAcceptorBuilder>,
}

impl Config {
    pub fn with_host(host: Url) -> Self {
        Self { host, ssl: None }
    }
}
