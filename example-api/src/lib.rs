use hsr_runtime::futures3::future::BoxFuture;

pub mod my_api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

use my_api::{CreatePetError, Error, NewPet, Pet, Pets};

pub struct Api;

impl my_api::Api for Api {
    fn new() -> Self {
        Api
    }
    fn get_all_pets(&self, limit: Option<i64>) -> BoxFuture<Pets> {
        unimplemented!()
    }
    fn create_pet(&self, new_pet: NewPet) -> BoxFuture<std::result::Result<(), CreatePetError>> {
        unimplemented!()
    }
    fn get_pet(&self, pet_id: i64) -> BoxFuture<std::result::Result<Pet, Error>> {
        unimplemented!()
    }
}
