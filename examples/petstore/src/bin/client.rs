#![feature(async_await)]

use petstore::{NewPet, api::PetstoreApi, client::Client};
use hsr::futures3::{FutureExt, TryFutureExt};

// Run the client via a CLI
fn main() {
    env_logger::init();
    let client = Client::new("http://localhost:8000".parse().unwrap());
    let fut = run(&client);
    hsr::actix_rt::System::new("main").block_on(fut.boxed_local().compat()).unwrap();
}

fn dbg(v: impl std::fmt::Debug) -> String {
    format!("{:?}", v)
}

async fn run(client: &Client) -> Result<(), String> {
    // Create two pets
    let pet1 = NewPet {
        name: "Alex the Goat".into(),
        tag: None
    };
    let pet2 = NewPet {
        name: "Bob the Badger".into(),
        tag: None
    };
    let () = client.create_pet(pet1).map_err(dbg).await?;
    let () = client.create_pet(pet2).map_err(dbg).await?;

    // Fetch a pet
    let pet = client.get_pet(0).map_err(dbg).await?;
    println!("{:?}", pet);

    // Fetch all pets
    let pets = client.get_all_pets(10, None).map_err(dbg).await?;
    println!("{:?}", pets);

    Ok(())
}
