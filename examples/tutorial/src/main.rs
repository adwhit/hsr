use tutorial::{api, Api};

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let config = hsr::Config::with_host("http://localhost:8000".parse().unwrap());
    api::server::serve(Api, config).await
}
