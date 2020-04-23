use test::api::{self, client, server, TestApi};

struct Api;

#[hsr::async_trait::async_trait(?Send)]
impl TestApi for Api {
    async fn status(&self) -> api::Status {
        api::Status::Ok
    }

    async fn one_param(&self, name: String) -> api::OneParam {
        api::OneParam::Ok(name)
    }

    async fn echo_name_and_age(&self, name: String, age: i64) -> api::EchoNameAndAge {
        api::EchoNameAndAge::Ok(api::Hello { name, age })
    }
}

// TODO make this into a 'normal' rust test suite not just a big main function

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    println!("Testing endpoints");

    println!("/status");
    client.status().await?;

    println!("/oneParam/{{name}}");
    let name = client.one_param("Alex".into()).await?;
    assert_eq!(name, api::OneParam::Ok("Alex".into()));

    println!("/echo/{{name}}/{{age}}");
    let echo = client.echo_name_and_age("Uncle Al".to_string(), 33).await?;

    assert_eq!(
        echo,
        api::EchoNameAndAge::Ok(api::Hello {
            name: "Uncle Al".into(),
            age: 33
        })
    );

    Ok(())
}
