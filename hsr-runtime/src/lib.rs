use std::fmt;

use futures::compat::*;
use futures::prelude::{Future, TryFuture, TryFutureExt, FutureExt, Stream, StreamExt, TryStream, TryStreamExt};
use hyper::{Body, Chunk, Request, Response};
use path_tree::PathTree;

pub fn serve() {
    unimplemented!()
}

struct Pet;
struct NewPet;

#[derive(Debug, Clone, Copy)]
struct Error;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error!")
    }
}

impl fmt::Display for Pet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Pet")
    }
}

impl std::error::Error for Error {}

trait Api {
    type R1: Future<Output = Result<Pet, Error>> + Unpin + Send + 'static;
    type R2: Future<Output = Result<Pet, Error>> + Unpin + Send + 'static;
    fn new() -> Self;
    fn get_pet(&self, pet_id: u32) -> Self::R1;
    fn create_pet(&self, pet: NewPet) -> Self::R2;
}

struct Handler<T: Api> {
    api: T,
}

impl<T: Api> Handler<T> {
    fn new() -> Self {
        Handler {
            api: T::new()
        }
    }

    fn respond(&self, req: Request<Body>) -> Response<Body> {
        let key = format!("{}{}", req.method(), req.uri().path());
        if let Some((token, matches)) = self.router.find(&key) {
            match token {
                0 => {
                    // Get the pet id from the route and call get_pet
                    let pet_id = (matches[0].1).parse().unwrap();
                    to_response(self.api.get_pet(pet_id))
                }
                1 => {
                    let (_, body): (_, Body) = req.into_parts();
                    body.compat().try_concat();
                    to_response(self.api.create_pet(unimplemented!()))
                }
                _ => unimplemented!(),
            }
        } else {
            unimplemented!()
        }
    }
}


fn to_response<F, T>(fut: F) -> Response<Body>
where
    F: Future<Output = Result<T, Error>> + Unpin + Send + 'static,
    T: fmt::Display
{
    let stream3 = fut.map_ok(|t| Chunk::from(t.to_string())).into_stream();
    let stream1 = stream3.compat();
    let body = Body::wrap_stream(stream1);
    Response::builder().body(body).unwrap()
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
