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

    fn greet(&self, name: String) -> hsr::HsrFuture<Result<Hello, api::GreetError<Self::Error>>> {
        hsr::wrap(Ok(Hello {
            name,
            greeting: "Pleased to meet you".into(),
        }))
    }
}

fn main() {
    use hsr::futures3::TryFutureExt;

    let uri: hsr::Url = "http://127.0.0.1:8000".parse().unwrap();
    let uri2 = uri.clone();
    std::thread::spawn(move || {
        println!("Serving at '{}'", uri);
        api::server::serve::<Api>(hsr::Config::with_host(uri)).unwrap();
    });

    std::thread::sleep(std::time::Duration::from_millis(100));

    let client = api::client::Client::new(uri2);
    println!("Querying server");
    let fut = client.greet("Bobert".to_string());
    let res = hsr::actix_rt::System::new("main").block_on(fut.compat());
    println!("Client response: {:?}", res);
}
