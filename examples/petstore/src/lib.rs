use hsr::futures::lock;
use regex::Regex;

pub mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

pub use api::{client, server, Error, NewPet, Pet, PetstoreApi};

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
}

type ApiResult<T> = std::result::Result<T, InternalError>;

// We define an object against which to implement our API trait
pub struct Api {
    database: lock::Mutex<Vec<Pet>>,
}

// // We simulate some kind of database interactions
impl Api {
    pub fn new() -> Self {
        Api {
            database: lock::Mutex::new(vec![]),
        }
    }

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
        Ok(())
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
    async fn get_all_pets(&self, limit: i64, filter: Option<String>) -> api::GetAllPets {
        let regex = if let Some(filter) = filter {
            match Regex::new(&filter) {
                Ok(re) => re,
                Err(_) => return api::GetAllPets::BadRequest,
            }
        } else {
            Regex::new(".?").unwrap()
        };
        let pets = match self.all_pets().await {
            Ok(p) => p,
            Err(_) => return api::GetAllPets::BadRequest,
        };
        api::GetAllPets::Ok(
            pets.into_iter()
                .take(limit as usize)
                .filter(|p| regex.is_match(&p.name))
                .collect(),
        )
    }

    async fn create_pet(&self, new_pet: NewPet) -> api::CreatePet {
        let res: ApiResult<()> = async {
            let () = self.server_health_check()?;
            let _ = self.add_pet(new_pet).await?; // TODO return usize
            Ok(())
        }
        .await;
        match res {
            Ok(()) => api::CreatePet::Created,
            Err(_) => api::CreatePet::Forbidden,
        }
    }

    async fn get_pet(&self, pet_id: i64) -> api::GetPet {
        match self.lookup_pet(pet_id as usize).await {
            Ok(Some(pet)) => api::GetPet::Ok(pet),
            Ok(None) => api::GetPet::NotFound,
            Err(_) => api::GetPet::Default {
                status_code: 500,
                body: api::Error {
                    code: 12345,
                    message: "Something went wrong".into(),
                },
            },
        }
    }

    async fn delete_pet(&self, pet_id: i64) -> api::DeletePet {
        match self.remove_pet(pet_id as usize).await {
            Ok(Some(_)) => api::DeletePet::NoContent,
            Ok(None) => api::DeletePet::NotFound,
            Err(_) => api::DeletePet::Default {
                status_code: 500,
                body: api::Error {
                    code: 12345,
                    message: "Something went wrong".into(),
                },
            },
        }
    }
}
