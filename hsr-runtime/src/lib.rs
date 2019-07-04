use std::fmt;

// use futures::compat::*;
use futures::prelude::*;
use futures1::prelude::Future as Future1;
// use hyper::{Body, Chunk, Request, Response};
use serde::{Serialize, Deserialize};
use warp::{self, path, Filter};

#[derive(Debug, Clone, Copy)]
struct Error;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error!")
    }
}

impl std::error::Error for Error {}

// Below should be code-genned

#[derive(Serialize)]
struct Pet;

#[derive(Deserialize)]
struct NewPet;

trait Api: Send + Sync {
    type R1: Future<Output = Result<Pet, Error>> + Unpin + Send + 'static;
    type R2: Future<Output = Result<Pet, Error>> + Unpin + Send + 'static;
    type R3: Future<Output = Result<Vec<Pet>, Error>> + Unpin + Send + 'static;
    fn new() -> Self;
    fn get_pet(&self, pet_id: u32) -> Self::R1;
    fn create_pet(&self, pet: NewPet) -> Self::R2;
    fn get_all(&self) -> Self::R3;
}

fn respond<A: Api + 'static>() -> impl Filter + Send + Sync + 'static {
    let api = std::sync::Arc::new(A::new());
    let api1 = api.clone();
    let api2 = api.clone();
    warp::get2()
        .and(path!["pet" / u32].and_then(move |id| {
            api1.get_pet(id)
                .compat()
                .map(|p| warp::reply::json(&p))
                .map_err(|e| warp::reject::custom(e))
        }))
        // .and(path!["pet"].and_then(move || {
        //     api2.clone()
        //         .get_all()
        //         .compat()
        //         .map(|p| warp::reply::json(&p))
        //         .map_err(|e| warp::reject::custom(e))
        // }))
}

fn serve() {
    warp::serve(respond());
}
