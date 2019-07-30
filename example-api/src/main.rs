#![feature(async_await)]

use hsr_runtime::futures3::{
    future::{FutureExt},
    lock,
};
use hsr_runtime::LocalBoxFuture3;
use regex::Regex;

pub mod pet_api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

use pet_api::{
    server, CreatePetError, Error, GetAllPetsError, GetPetError, NewPet, Pet, Pets, PetstoreApi,
};

impl Pet {
    fn new(id: i64, name: String, tag: Option<String>) -> Pet {
        Pet { id, name, tag }
    }
}

// An error to be used internally. Generally if this
// gets returned from the top level, we just want to return "HTTP 500"
pub enum InternalError {
    BadConnection,
    ParseFailure,
    ServerHasExploded,
}

// Boilerplate impls necessary to fulfil API contract
impl hsr_runtime::HasStatusCode for InternalError {}
impl hsr_runtime::Error for InternalError {}

// TODO is it possible to remove the requirement for this impl?
// Alternatively, add a trait bound for HasStatusCode to give a
// nicer error?
impl hsr_runtime::HasStatusCode for Error {}

type ApiResult<T> = std::result::Result<T, InternalError>;

// We define an object against which to implement our API trait
pub struct Api {
    database: lock::Mutex<Vec<Pet>>,
}

// We simulate some kind of database interactions
impl Api {
    async fn connect_db(&self) -> ApiResult<lock::MutexGuard<Vec<Pet>>> {
        if rand::random::<f32>() > 0.8 {
            Err(InternalError::BadConnection)
        } else {
            Ok(self.database.lock().await)
        }
    }

    async fn all_pets(&self) -> ApiResult<Vec<Pet>> {
        let db = self.connect_db().await?;
        Ok(db.clone())
    }

    async fn lookup_pet(&self, id: usize) -> ApiResult<Option<Pet>> {
        let db = self.connect_db().await?;
        Ok(db.get(id).cloned())
    }

    async fn add_pet(&self, new_pet: NewPet) -> ApiResult<usize> {
        let mut db = self.connect_db().await?;
        let id = db.len();
        let new_pet = Pet::new(id as i64, new_pet.name, new_pet.tag);
        db.push(new_pet);
        Ok(id)
    }

    fn server_health_check(&self) -> ApiResult<()> {
        if rand::random::<f32>() > 0.99 {
            Err(InternalError::ServerHasExploded)
        } else {
            Ok(())
        }
    }
}

// The meat of the example. We fulfill the server interface as defined by the
// `petstore.yaml` OpenAPI file by implementing the PetstoreApi trait.
//
// The trait function definitions may not be obvious just from reading the spec,
// in which case it will be helpful to run `cargo doc` to see the trait rendered
// by `rustdoc`. (Of course, if the trait is not implemented correcty, it will
// not compile).
impl PetstoreApi for Api {
    type Error = InternalError;

    fn new(_uri: hsr_runtime::Uri) -> Self {
        Api {
            database: lock::Mutex::new(vec![]),
        }
    }

    // TODO all these i64s should be u64s
    fn get_all_pets(
        &self,
        limit: i64,
        filter: Option<String>,
    ) -> LocalBoxFuture3<Result<Pets, GetAllPetsError<Self::Error>>> {
        async move {
            let regex = if let Some(filter) = filter {
                Regex::new(&filter).map_err(|_| GetAllPetsError::BadRequest)?
            } else {
                Regex::new(".?").unwrap()
            };
            let pets = self.all_pets().await?;
            Ok(pets
                .into_iter()
                .take(limit as usize)
                .filter(|p| regex.is_match(&p.name))
                .collect())
        }
            .boxed()
    }

    fn create_pet(&self, new_pet: NewPet) -> LocalBoxFuture3<Result<(), CreatePetError<Self::Error>>> {
        async move {
            let () = self.server_health_check()?;
            let _ = self.add_pet(new_pet).await?; // TODO return usize
            Ok(())
        }
            .boxed()
    }

    fn get_pet(&self, pet_id: i64) -> LocalBoxFuture3<Result<Pet, GetPetError<Self::Error>>> {
        // TODO This is how we would like it to work
        async move {
            self.lookup_pet(pet_id as usize)
                .await?
                .ok_or_else(|| GetPetError::NotFound)
        }
            .boxed()
    }
}

// Serve the API.
//
// Navigate your browser to http://localhost:8000/ui.html to see
// the API as rendered by [Swagger UI](https://github.com/swagger-api/swagger-ui)
fn main() -> Result<(), std::io::Error> {
    let uri = "localhost:8000".parse().unwrap();
    server::serve::<Api>(uri)
}
