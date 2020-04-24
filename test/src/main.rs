use test::api::{self, client, server, TestApi};

struct Api;

#[hsr::async_trait::async_trait(?Send)]
impl TestApi for Api {
    async fn get_status(&self) -> api::GetStatus {
        api::GetStatus::Ok
    }

    async fn set_status(&self, status: Option<String>) -> api::SetStatus {
        api::SetStatus::Ok(status)
    }

    async fn two_path_params(&self, name: String, age: i64) -> api::TwoPathParams {
        api::TwoPathParams::Ok(api::Hello {
            myName: name,
            my_age: Some(age),
        })
    }

    async fn two_query_params(&self, myName: String, my_age: Option<i64>) -> api::TwoQueryParams {
        api::TwoQueryParams::Ok(api::Hello { myName, my_age })
    }

    async fn just_default(&self) -> api::JustDefault {
        api::JustDefault::Default {
            status_code: 200,
            body: hello(),
        }
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
        my_age: Some(33),
    }
}

#[allow(dead_code)]
fn nullable_struct() -> api::NullableStruct {
    Some(api::NullableStructOpt {
        this: "string".into(),
        that: Some(123),
        other: Some(vec!["string".into()]),
        flooglezingle: Some(true),
    })
}

fn combination() -> api::Combination {
    let blob = serde_json::json!({
        "myName": "Alex",
        "height": 1.88,
        "feet_info": {
            "number_of_toes": 6,
            "webbed": true,
            "the_rest": "blah blah blah blah"
        }
    });
    serde_json::from_value(blob).unwrap()
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

    let _ = combination();

    assert_eq!(client.get_status().await?, api::GetStatus::Ok);

    {
        assert_eq!(
            client.set_status(Some("some-status".into())).await?,
            api::SetStatus::Ok(Some("some-status".into()))
        );
        assert_eq!(client.set_status(None).await?, api::SetStatus::Ok(None));
    }

    {
        let echo = client.two_path_params("Alex".to_string(), 33).await?;
        assert_eq!(echo, api::TwoPathParams::Ok(hello()));
    }

    {
        let echo = client
            .two_query_params("Alex".to_string(), Some(33))
            .await?;
        assert_eq!(echo, api::TwoQueryParams::Ok(hello()));

        let echo = client.two_query_params("Alex".to_string(), None).await?;
        assert_eq!(
            echo,
            api::TwoQueryParams::Ok(api::Hello {
                myName: "Alex".into(),
                my_age: None
            })
        );
    }

    {
        let rtn = client.just_default().await?;
        assert_eq!(
            rtn,
            api::JustDefault::Default {
                status_code: 200,
                body: hello()
            }
        )
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
