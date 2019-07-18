pub use actix_web;
pub use futures1;
pub use futures3;
pub use serde;

pub trait HasStatusCode {
    fn get_status_code(&self) -> actix_web::http::StatusCode;
}
