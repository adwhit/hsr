mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

use api::{Hello, QuickstartApi};

struct Api;

#[hsr::async_trait::async_trait(?Send)]
impl QuickstartApi for Api {
    type Error = hsr::ServerError;

    fn new(_url: hsr::Url) -> Self {
        Api
    }

    async fn greet(&self, name: String) -> Result<Hello, api::GreetError<Self::Error>> {
        Ok(Hello {
            name,
            greeting: "Pleased to meet you".into(),
        })
    }
}

#[actix_rt::main]
async fn main() -> Result<(), api::GreetError<hsr::ClientError>> {
    env_logger::init();

    let uri: hsr::Url = "http://127.0.0.1:8000".parse().unwrap();
    let uri2 = uri.clone();

    std::thread::spawn(move || {
        println!("Serving at '{}'", uri);
        let mut system = hsr::actix_rt::System::new("main");
        let server = api::server::server::<Api>(hsr::Config::with_host(uri)).unwrap();
        system.block_on(server).unwrap();
        println!("Died");
    });

    std::thread::sleep(std::time::Duration::from_millis(100));

    let client = api::client::Client::new(uri2);
    println!("Querying server");
    let greeting = client.greet("Bobert".to_string()).await?;
    println!("{:?}", greeting);
    Ok(())
}
