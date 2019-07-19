pub use actix_web;
pub use futures1;
pub use futures3;
pub use serde;

use actix_web::{HttpRequest, HttpResponse, Responder, Either};
use std::fmt;

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

pub trait HasStatusCode {
    fn get_status_code(&self) -> actix_web::http::StatusCode;
}

pub fn result_to_either<A, B>(res: Result<A, B>) -> Either<A, B> {
    match res {
        Ok(a) => Either::A(a),
        Err(b) => Either::B(b),
    }
}
