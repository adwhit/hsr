use quickstart::api::{client, server, Greet, Hello, QuickstartApi};

struct Api;

#[hsr::async_trait::async_trait(?Send)]
impl QuickstartApi for Api {
    async fn greet(&self, name: String) -> Greet {
        Greet::Ok(Hello {
            name,
            greeting: "Pleased to meet you".into(),
        })
    }
}

#[actix_rt::main]
async fn main() {
    env_logger::init();

    let uri: hsr::Url = "http://127.0.0.1:8000".parse().unwrap();
    let uri2 = uri.clone();

    std::thread::spawn(move || {
        println!("Serving at '{}'", uri);
        let mut system = hsr::actix_rt::System::new("main");
        let server = server::serve(Api, hsr::Config::with_host(uri));
        system.block_on(server).unwrap();
    });

    std::thread::sleep(std::time::Duration::from_millis(100));

    let client = client::Client::new(uri2);
    println!("Querying server");
    let greeting = client.greet("Bobert".to_string()).await;
    println!("{:?}", greeting);
}
