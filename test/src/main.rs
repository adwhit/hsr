use test::api::{self, client, server, TestApi};

struct Api;

#[hsr::async_trait::async_trait(?Send)]
impl TestApi for Api {
    async fn status(&self) -> api::Status {
        api::Status::Ok
    }

    async fn two_path_params(&self, name: String, age: i64) -> api::TwoPathParams {
        api::TwoPathParams::Ok(api::Hello {
            myName: name,
            my_age: age,
        })
    }

    async fn two_query_params(&self, myName: String, my_age: i64) -> api::TwoQueryParams {
        api::TwoQueryParams::Ok(api::Hello { myName, my_age })
    }

    async fn just_default(&self) -> api::JustDefault {
        api::JustDefault::Default { status_code: 200, body: hello() }
    }

    async fn ok_error_default(&self, return_code: i64) -> api::OkErrorDefault {
        match return_code {
            200 => api::OkErrorDefault::Ok,
            400 => api::OkErrorDefault::BadRequest,
            other => api::OkErrorDefault::Default {
                status_code: other as u16,
            },
        }
    }

    async fn nestedResponse(&self) -> api::NestedResponse {
        api::NestedResponse::Ok(api::PathsNestedResponseTypeGetResponses {
            first: api::PathsNestedResponseTypeGetResponsesFirst {
                second: api::PathsNestedResponseTypeGetResponsesFirstSecond {},
            },
        })
    }
}

// Quickly generate some data
fn hello() -> api::Hello {
    api::Hello {
        myName: "Alex".into(),
        my_age: 333
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
                myName: "Uncle Al".into(),
                my_age: 33
            })
        );
    }

    {
        let echo = client.two_query_params("Uncle Al".to_string(), 33).await?;
        assert_eq!(
            echo,
            api::TwoQueryParams::Ok(api::Hello {
                myName: "Uncle Al".into(),
                my_age: 33
            })
        );
    }

    {
        let rtn = client.just_default().await?;
        assert_eq!(rtn, api::JustDefault::Default { status_code: 200, body: hello() })
    }

    {
        let rtn = client.ok_error_default(200).await?;
        assert_eq!(rtn, api::OkErrorDefault::Ok);

        let rtn = client.ok_error_default(400).await?;
        assert_eq!(rtn, api::OkErrorDefault::BadRequest);

        let rtn = client.ok_error_default(201).await?;
        assert_eq!(rtn, api::OkErrorDefault::Default { status_code: 201 });

        let rtn = client.ok_error_default(-1).await?;
        assert_eq!(rtn, api::OkErrorDefault::Default { status_code: 500 });
    }

    {
        let nested = client.nestedResponse().await?;
        assert_eq!(
            nested,
            api::NestedResponse::Ok(api::PathsNestedResponseTypeGetResponses {
                first: api::PathsNestedResponseTypeGetResponsesFirst {
                    second: api::PathsNestedResponseTypeGetResponsesFirstSecond {}
                }
            })
        );
    }

    println!("Success");

    Ok(())
}
