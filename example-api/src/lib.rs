#![feature(async_await)]

use hsr_runtime::futures3::future::{BoxFuture, FutureExt};
use regex::Regex;
use std::sync::Mutex;

pub mod my_api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

use my_api::*;

impl Pet {
    fn new(id: i64, name: String, tag: Option<String>) -> Pet {
        Pet { id, name, tag }
    }
}

pub struct Api {
    database: Mutex<Vec<Pet>>,
}

pub enum InternalError {
    BadConnection,
    ParseFailure,
    ServerHasExploded,
}

impl hsr_runtime::HasStatusCode for InternalError {}
impl hsr_runtime::HasStatusCode for Error {}
impl hsr_runtime::Error for InternalError {}

type ApiResult<T> = std::result::Result<T, InternalError>;

impl Api {
    fn all_pets(&self) -> ApiResult<Vec<Pet>> {
        if rand::random::<f32>() > 0.6 {
            Err(InternalError::BadConnection)
        } else {
            Ok(self.database.lock().unwrap().clone())
        }
    }

    fn lookup_pet(&self, id: usize) -> ApiResult<Option<Pet>> {
        if rand::random::<f32>() > 0.8 {
            Err(InternalError::BadConnection)
        } else {
            Ok(self.database.lock().unwrap().get(id).cloned())
        }
    }

    fn add_pet(&self, new_pet: NewPet) -> ApiResult<usize> {
        if rand::random::<f32>() > 0.8 {
            Err(InternalError::BadConnection)
        } else {
            let mut pets = self.database.lock().unwrap();
            let id = pets.len();
            let new = Pet::new(id as i64, new_pet.name, new_pet.tag);
            pets.push(new);
            Ok(id)
        }
    }

    fn server_health_check(&self) -> ApiResult<()> {
        if rand::random::<f32>() > 0.99 {
            Err(InternalError::ServerHasExploded)
        } else {
            Ok(())
        }
    }
}

impl my_api::PetstoreApi for Api {
    type Error = InternalError;

    fn new() -> Self {
        Api {
            database: Mutex::new(vec![]),
        }
    }

    // TODO all these i64s should be u64s
    fn get_all_pets(
        &self,
        filter: Option<String>,
        limit: i64,
    ) -> BoxFuture<Result<Pets, GetAllPetsError<Self::Error>>> {
        async move {
            let regex = if let Some(filter) = filter {
                Regex::new(&filter).map_err(|_| GetAllPetsError::BadRequest)?
            } else {
                Regex::new(".?").unwrap()
            };
            let pets = self.all_pets()?;
            Ok(pets
                .into_iter()
                .take(limit as usize)
                .filter(|p| regex.is_match(&p.name))
                .collect())
        }
            .boxed()
    }

    fn create_pet(&self, new_pet: NewPet) -> BoxFuture<Result<(), CreatePetError<Self::Error>>> {
        async move {
            let () = self.server_health_check()?;
            Ok(self.add_pet(new_pet).map(|_| ())?) // TODO return usize
        }
            .boxed()
    }

    fn get_pet(&self, pet_id: i64) -> BoxFuture<Result<Pet, GetPetError<Self::Error>>> {
        // TODO This is how we would like it to work
        async move {
            self.lookup_pet(pet_id as usize)?
             .ok_or_else(|| GetPetError::NotFound)
        }
            .boxed()
    }
}
