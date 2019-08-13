#![feature(async_await)]

mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

use api::{Hello, QuickstartApi};

struct Api;

impl QuickstartApi for Api {
    type Error = hsr::ServerError;

    fn new(_url: hsr::Url) -> Self {
        Api
    }

    fn index(&self, name: String) -> hsr::HsrFuture<Result<Hello, api::IndexError<Self::Error>>> {
        hsr::wrap(Ok(Hello {
            name,
            greeting: "Pleased to meet you".into(),
        }))
    }
}

fn main() -> Result<(), std::io::Error> {
    let uri = "http://127.0.0.1:8000".parse().unwrap();
    api::server::serve::<Api>(uri)
}
