use futures::prelude::{Future, FutureExt, Stream, StreamExt, TryStream, TryStreamExt};
use futures::compat::*;
use hyper::{Request, Response, Body};
use path_tree::PathTree;

pub fn serve() {
    unimplemented!()
}

struct Pet;
struct NewPet;
struct Error;

trait Api {
    type R1: Future<Output=Result<Pet, Error>>;
    type R2: Future<Output=Result<Pet, Error>>;
    fn new() -> Self;
    fn get_pet(&self, pet_id: u32) -> Self::R1;
    fn create_pet(&self, pet: NewPet) -> Self::R2;
}

struct Handler<T: Api> {
    router: PathTree<u32>,
    api: T
}

impl<T: Api> Handler<T> {
    fn new() -> Self {
        let mut router = PathTree::new();
        router.insert("GET/pet/:petId", 0);
        router.insert("POST/pet", 1);
        Self {
            router,
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
                    let pet_fut = self.api.get_pet(pet_id).
                        map(|pet| {
                            let pet_str: String = unimplemented!();
                            pet_str
                        });
                    Response::builder()
                        .body(pet_fut)
                        .unwrap()
                }
                1 => {
                    let (_, body): (_, Body) = req.into_parts();
                    body.compat().try_concat();
                    unimplemented!()
                },
                _ => unimplemented!()

            }
        } else {
            unimplemented!()
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
