#![feature(async_await)]

use hsr_runtime::futures3::future::{BoxFuture, FutureExt};
use regex::Regex;
use std::sync::Mutex;

pub mod my_api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

use my_api::*;

pub struct Api {
    database: Mutex<Vec<Pet>>,
}

// There is a need to distinguish between external and internal errors
// External errors usually come from bad authorization, malformed data or resource not found
// These are exposed to the user as 4XX errors and are part of our API - they are NOT
// 'Errors' as such, or at least, they do not indicate a problem with our server.
// Moreover, the server will handle a lot of these automatically, e.g. if route is not
// found it will automatically return 404.

// On the other hamd, plenty of things can go wrong internally too, e.g. database connection failed,
// connection timeout, parse failures and so on. Usually it is nicest to handle these with the `?` operator

enum InternalError {
    BadConnection,
    ParseFailure,
    ServerHasExploded
}

impl Api {
    fn lookup_pet(&self, id: usize) -> Result<Option<Pet>, InternalError> {
        if rand::random::<f32>() > 0.6 {
            Err(InternalError::BadConnection)
        } else {
            Ok(self.database.lock().unwrap().get(id).cloned())
        }
    }

    fn add_pet(&self, new_pet: NewPet) -> Result<usize, InternalError> {
        if rand::random::<f32>() > 0.6 {
            Err(InternalError::BadConnection)
        } else {
            let mut pets = self.database.lock().unwrap();
            let id = pets.len();
            let new = Pet::new(id, new_pet.name, new_pet.tag);
            pets.push(new);
            Ok(id)
        }
    }

    fn server_health_check(&self) -> Result<(), InternalError> {
        if rand::random::<f32>() > 0.99 {
            Err(InternalError::ServerHasExploded)
        } else {
            Ok(())
        }
    }
}

impl Pet {
    fn new(id: i64, name: String, tag: Option<String>) -> Pet {
        Pet { id, name, tag }
    }
}

impl my_api::Api for Api {
    fn new() -> Self {
        Api {
            database: Mutex::new(vec![]),
        }
    }

    // TODO all these i64s should be u64s
    fn get_all_pets(&self, filter: Option<String>, limit: i64) -> BoxFuture<Pets> {
        async move {
            let regex = if let Some(filter) = filter {
                Regex::new(&filter).unwrap()
            } else {
                Regex::new(".?").unwrap()
            };
            let pets = self.database.lock().unwrap();
            pets.iter()
                .take(limit as usize)
                .filter(|p| regex.is_match(&p.name))
                .cloned()
                .collect()
        }
            .boxed()
    }

    fn create_pet(&self, new_pet: NewPet) -> BoxFuture<Result<(), CreatePetError>> {
        async move {
            let () = self.server_health_check()?;
            let mut pets = self.database.lock().unwrap();
            let new = Pet::new(pets.len() as i64, new_pet.name, new_pet.tag);
            pets.push(new);
            Ok(())
        }
            .boxed()
    }

    fn get_pet(&self, pet_id: i64) -> BoxFuture<std::result::Result<Pet, GetPetError>> {
        // TODO This is how we would like it to work
        async move {
            self.lookup_pet(pet_id as usize)?
                .ok_or_else(|| GetPetError::NotFound)
        }
        .boxed()
    }
}
