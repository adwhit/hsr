#![feature(async_await)]

use hsr_runtime::futures3::future::{BoxFuture, FutureExt};
use regex::Regex;
use std::sync::Mutex;

pub mod my_api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

use my_api::*;

pub struct Api {
    pets: Mutex<Vec<Pet>>,
}

impl Pet {
    fn new(id: i64, name: String, tag: Option<String>) -> Pet {
        Pet { id, name, tag }
    }
}

impl my_api::Api for Api {
    fn new() -> Self {
        Api {
            pets: Mutex::new(vec![]),
        }
    }

    // TODO all these i64s should be u64s
    fn get_all_pets(&self, limit: i64, filter: Option<String>) -> BoxFuture<Pets> {
        async move {
            let regex = if let Some(filter) = filter {
                Regex::new(&filter).unwrap()
            } else {
                Regex::new(".?").unwrap()
            };
            let pets = self.pets.lock().unwrap();
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
            let mut pets = self.pets.lock().unwrap();
            let new = Pet::new(pets.len() as i64, new_pet.name, new_pet.tag);
            pets.push(new);
            Ok(())
        }
            .boxed()
    }

    fn get_pet(&self, pet_id: i64) -> BoxFuture<std::result::Result<Pet, Error>> {
        async move {
            let pets = self.pets.lock().unwrap();
            pets.get(pet_id as usize).cloned().ok_or(Error {
                code: 10101,
                message: "Not found".into(),
            })
        }
            .boxed()
    }
}
