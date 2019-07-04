use std::fmt;

use std::sync::Arc;

use futures::prelude::*;
use futures1::prelude::Future as Future1;
use serde::{Deserialize, Serialize};

use actix_web::error::ResponseError;
use actix_web::web::{self, Data, Json as AxJson, Path as AxPath};
use actix_web::{App, HttpServer};

#[derive(Debug, Clone, Copy)]
pub struct Error;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error!")
    }
}

impl std::error::Error for Error {}
impl ResponseError for Error {}

// ***************************************
// ***   Below should be code-genned   ***
// ***************************************

#[derive(Serialize)]
pub struct Pet;

#[derive(Deserialize)]
pub struct NewPet;

pub trait Api: Send + Sync + 'static {
    type R1: Future<Output = Result<Pet, Error>> + Unpin + Send + Sync + 'static;
    type R2: Future<Output = Result<Pet, Error>> + Unpin + Send + 'static;
    type R3: Future<Output = Result<Vec<Pet>, Error>> + Unpin + Send + 'static;
    fn new() -> Self;
    fn get_pet(&self, pet_id: u32) -> Self::R1;
    fn create_pet(&self, pet: NewPet) -> Self::R2;
    fn get_all(&self) -> Self::R3;
}

fn get_pet<A: Api>(
    data: Data<Arc<A>>,
    path: AxPath<u32>,
) -> impl Future1<Item = AxJson<Pet>, Error = Error> {
    (*data).get_pet(path.into_inner()).map_ok(AxJson).compat()
}

fn create_pet<A: Api>(
    data: Data<Arc<A>>,
    json: AxJson<NewPet>,
) -> impl Future1<Item = AxJson<Pet>, Error = Error> {
    (*data)
        .create_pet(json.into_inner())
        .map_ok(AxJson)
        .compat()
}

pub fn serve<A: Api>(api: A) -> std::io::Result<()> {
    let api = Arc::new(api);
    HttpServer::new(move || {
        let app = App::new()
            .data(api.clone())
            .service(
            web::resource("/pets/{petId}")
                .route(web::get().to_async(get_pet::<A>))
                .route(web::post().to_async(create_pet::<A>)),
        );
        app
    })
    .bind("127.0.0.1:8000")?
    .run()
}

// ***************************************
// ***   Below should be user-impled   ***
// ***************************************

struct MyApi;

impl Api for MyApi {
}
