mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}

struct Api;

#[hsr::async_trait::async_trait(?Send)]
impl api::BenchmarkApi for Api {
    type Error = hsr::ServerError;

    fn new(_url: hsr::Url) -> Self {
        Api
    }

    async fn basic_get(&self) -> Result<hsr::Success, api::BasicGetError<Self::Error>> {
        Ok(hsr::Success)
    }

    async fn basic_post(
        &self,
        payload: api::Payload,
    ) -> Result<api::Payload, api::BasicPostError<Self::Error>> {
        Ok(payload)
    }
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let uri: hsr::Url = "http://127.0.0.1:8000".parse().unwrap();
    println!("Serving at '{}'", uri);
    api::server::serve::<Api>(hsr::Config::with_host(uri)).await
}
