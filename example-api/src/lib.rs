use std::future::Future;

use my_api;

struct Api;

impl my_api::Api for Api {
    fn new() -> Self {
        Api
    }

    fn show_pet_by_id(&self, test: my_api::Test) -> Box<Future<Output = my_api::Test>> {
        unimplemented!()
    }

    fn list_pets(&self, test: my_api::Test) -> Box<Future<Output = my_api::Test>> {
        unimplemented!()
    }
}
