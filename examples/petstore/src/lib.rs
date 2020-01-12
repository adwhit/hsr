use hsr::futures::lock;
use regex::Regex;

pub mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

pub use api::{client, server, NewPet, Pet, Pets, PetstoreApi};
use api::{CreatePetError, DeletePetError, Error, GetAllPetsError, GetPetError};

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
impl hsr::HasStatusCode for InternalError {}

// TODO is it possible to remove the requirement for this impl?
// Alternatively, add a trait bound for HasStatusCode to give a
// nicer error?
impl hsr::HasStatusCode for Error {}

type ApiResult<T> = std::result::Result<T, InternalError>;

// We define an object against which to implement our API trait
pub struct Api {
    database: lock::Mutex<Vec<Pet>>,
}

// We simulate some kind of database interactions
impl Api {
    async fn connect_db(&self) -> ApiResult<lock::MutexGuard<'_, Vec<Pet>>> {
        if rand::random::<f32>() > 0.95 {
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

    async fn remove_pet(&self, id: usize) -> ApiResult<Option<Pet>> {
        let mut db = self.connect_db().await?;
        if id < db.len() {
            Ok(Some(db.remove(id)))
        } else {
            Ok(None)
        }
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
#[hsr::async_trait::async_trait(?Send)]
impl PetstoreApi for Api {
    type Error = InternalError;

    fn new(_uri: hsr::url::Url) -> Self {
        Api {
            database: lock::Mutex::new(vec![]),
        }
    }

    // TODO all these i64s should be u64s
    async fn get_all_pets(
        &self,
        limit: i64,
        filter: Option<String>,
    ) -> Result<Pets, GetAllPetsError<Self::Error>> {
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

    async fn create_pet(
        &self,
        new_pet: NewPet,
    ) -> Result<hsr::Success, CreatePetError<Self::Error>> {
        let () = self.server_health_check()?;
        let _ = self.add_pet(new_pet).await?; // TODO return usize
        Ok(hsr::Success)
    }

    async fn get_pet(&self, pet_id: i64) -> Result<Pet, GetPetError<Self::Error>> {
        // TODO This is how we would like it to work
        self.lookup_pet(pet_id as usize)
            .await?
            .ok_or_else(|| GetPetError::NotFound)
    }

    async fn delete_pet(&self, pet_id: i64) -> Result<hsr::Success, DeletePetError<Self::Error>> {
        self.remove_pet(pet_id as usize)
            .await?
            .map(|_| hsr::Success)
            .ok_or_else(|| DeletePetError::NotFound)
    }
}
