use std::future::Future;

mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

struct Api;

impl api::Api for Api {
    fn new() -> Self { Api }

    fn show_pet_by_id(&self, test: api::Test) -> Box<Future<Output=api::Test>> {
        unimplemented!()
    }

    fn list_pets(&self, test: api::Test) -> Box<Future<Output=api::Test>> {
        unimplemented!()
    }
}

struct Handler<T: api::Api> {
    api: T
}
