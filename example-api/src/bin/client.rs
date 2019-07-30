#![feature(async_await)]

use example_api::{NewPet, pet_api::PetstoreApi, client::Client};
use hsr_runtime::futures3::{FutureExt, TryFutureExt};

// Run the client via a CLI
fn main() {
    env_logger::init();
    let client = Client::new("http://localhost:8000".parse().unwrap());
    let fut = run(&client);
    hsr_runtime::actix_rt::System::new("main").block_on(fut.boxed_local().compat()).unwrap();
}

fn dbg(v: impl std::fmt::Debug) -> String {
    format!("{:?}", v)
}

async fn run(client: &Client) -> Result<(), String> {
    let pet = NewPet {
        name: "Alex the Goat".into(),
        tag: None
    };
    let () = client.create_pet(pet).map_err(dbg).await?;
    let pet = client.get_pet(1).map_err(dbg).await?;
    println!("{:?}", pet);
    Ok(())
}
