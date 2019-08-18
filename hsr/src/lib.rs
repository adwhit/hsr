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
pub use awc;
pub use futures1;
pub use futures3;
pub use serde_urlencoded;
pub use url;

pub use openssl;

pub use url::Url;

// We re-export this type as it is used in all the trait functions
pub use futures3_core::future::LocalBoxFuture as HsrFuture;

pub fn wrap<'a, T: 'a>(out: T) -> HsrFuture<'a, T> {
    use futures3::FutureExt;
    futures3::future::ready(out).boxed_local()
}

use actix_http::http::StatusCode;
use actix_web::{Either, Error as AxError, HttpRequest, HttpResponse, Responder};
use std::fmt;

/// An empty type which cannot be instantiated.
///
/// Useful because Future1 demands that we return an Err type, even when
/// we don't want to or don't have one. Then we can return a Void, essentially
/// the same as the '!' type but with various trait impls
#[doc(hidden)]
pub enum Void {}

impl fmt::Debug for Void {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        unreachable!()
    }
}

impl fmt::Display for Void {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        unreachable!()
    }
}

impl Responder for Void {
    type Error = ();
    type Future = Result<HttpResponse, ()>;

    fn respond_to(self, _: &HttpRequest) -> Self::Future {
        unreachable!()
    }
}

impl actix_web::ResponseError for Void {}

/// Associate an http status code with a type. Defaults to 501 Internal Server Error
pub trait HasStatusCode {
    /// The http status code associated with the type
    fn status_code(&self) -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

#[doc(hidden)]
/// Helper function. Needs to be public because used 'implicitly' by hsr-codegen.
pub fn result_to_either<A, B>(res: Result<A, B>) -> Either<A, B> {
    match res {
        Ok(a) => Either::A(a),
        Err(b) => Either::B(b),
    }
}

/// Errors that may be returned by the client, apart from those explicitly
/// specified in the spec.
///
/// This will handle bad connections, path errors, unreconginized statuses
/// and any other 'unexpected errors'
#[derive(Debug)]
pub enum ClientError {
    BadStatus(StatusCode),
    Actix(AxError),
}

impl HasStatusCode for ClientError {}

/// An opaque error that may be returned to deliver a 500 Internal Server Error
#[derive(Debug, Clone, Copy)]
pub struct ServerError;

impl HasStatusCode for ServerError {}

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
