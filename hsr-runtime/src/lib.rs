pub use actix_web;
pub use futures1;
pub use futures3;
pub use serde;

use actix_web::Either;

pub enum Void {}

pub fn result_to_either<A, B>(res: Result<A, B>) -> Either<A, B> {
    match res {
        Ok(a) => Either::A(a),
        Err(b) => Either::B(b),
    }
}
