#![feature(async_await)]

mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

struct Api;

impl api::BenchmarkApi for Api {
    type Error = hsr::ServerError;

    fn new(_url: hsr::Url) -> Self {
        Api
    }

    fn basic(&self) -> hsr::HsrFuture<std::result::Result<(), api::BasicError<Self::Error>>> {
        hsr::wrap(Ok(()))
    }

}

fn main() {
    let uri: hsr::Url = "http://127.0.0.1:8000".parse().unwrap();
    println!("Serving at '{}'", uri);
    api::server::serve::<Api>(hsr::Config::with_host(uri)).unwrap();
}
