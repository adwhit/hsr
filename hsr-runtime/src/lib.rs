#![feature(async_await)]

use std::fmt;

use futures::prelude::*;
use futures::future::BoxFuture;
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
    fn new() -> Self;
    fn get_pet(&self, pet_id: u32) -> BoxFuture<Result<Pet, Error>>;
    fn create_pet(&self, pet: NewPet) -> BoxFuture<Result<Pet, Error>>;
    fn get_all_pets(&self) -> BoxFuture<Result<Vec<Pet>, Error>>;
}

fn get_pet<A: Api>(
    data: Data<A>,
    path: AxPath<u32>,
) -> impl Future1<Item = AxJson<Pet>, Error = Error> {
    async move {
        let pet = data.get_pet(path.into_inner()).await?;
        Ok(AxJson(pet))
    }.boxed().compat()
}

fn create_pet<A: Api>(
    data: Data<A>,
    json: AxJson<NewPet>,
) -> impl Future1<Item = AxJson<Pet>, Error = Error> {
    async move {
        let pet = data.create_pet(json.into_inner()).await?;
        Ok(AxJson(pet))
    }.boxed().compat()
}

fn get_all_pets<A: Api>(
    data: Data<A>,
) -> impl Future1<Item = AxJson<Vec<Pet>>, Error = Error> {
    async move {
        let pets = data.get_all_pets().await?;
        Ok(AxJson(pets))
    }.boxed().compat()
}

pub fn serve<A: Api>() -> std::io::Result<()> {
    let api = Data::new(A::new());
    HttpServer::new(move || {
        let app = App::new()
            .register_data(api.clone())
            .service(
                web::resource("/pets/{petId}")
                    .route(web::get().to_async(get_pet::<A>))
                    .route(web::post().to_async(create_pet::<A>)),
            ).route("/pets", web::get().to_async(get_all_pets::<A>));
        app
    })
        .bind("127.0.0.1:8000")?
        .run()
}

// ***************************************
// ***   Below should be user-impled   ***
// ***************************************

struct MyApi;

impl MyApi {
    async fn query(&self) -> Result<Pet, Error> {
        Ok(Pet)
    }
}

impl Api for MyApi {
    fn new() -> Self { MyApi }
    fn get_pet(&self, pet_id: u32) -> BoxFuture<Result<Pet, Error>> {
        futures::future::ok(Pet).boxed()
    }
    fn create_pet(&self, _pet: NewPet) -> BoxFuture<Result<Pet, Error>> {
        async { Ok(Pet) }.boxed()
    }
    fn get_all_pets(&self) -> BoxFuture<Result<Vec<Pet>, Error>> {
        async move {
            let pet = self.query().await?;
            Ok(vec![pet])
        }.boxed()
    }
}
