use test::api::{self, client, server, TestApi};

struct Api;

#[hsr::async_trait::async_trait(?Send)]
impl TestApi for Api {
    async fn status(&self) -> api::Status {
        api::Status::Ok
    }

    async fn two_path_params(&self, name: String, age: i64) -> api::TwoPathParams {
        api::TwoPathParams::Ok(api::Hello { name, age })
    }

    async fn two_query_params(&self, name: String, age: i64) -> api::TwoQueryParams {
        api::TwoQueryParams::Ok(api::Hello { name, age })
    }

    async fn nested_response(&self) -> api::NestedResponse {
        api::NestedResponse::Ok(api::PathsNestedResponseTypeGetResponses {
            first: api::PathsNestedResponseTypeGetResponsesFirst {
                second: api::PathsNestedResponseTypeGetResponsesFirstSecond {},
            },
        })
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

    client.status().await?;

    {
        let echo = client.two_path_params("Uncle Al".to_string(), 33).await?;
        assert_eq!(
            echo,
            api::TwoPathParams::Ok(api::Hello {
                name: "Uncle Al".into(),
                age: 33
            })
        );
    }

    {
        let echo = client.two_query_params("Uncle Al".to_string(), 33).await?;

        assert_eq!(
            echo,
            api::TwoQueryParams::Ok(api::Hello {
                name: "Uncle Al".into(),
                age: 33
            })
        );
    }

    println!("Success");

    Ok(())
}
