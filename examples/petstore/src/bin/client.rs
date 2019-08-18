#![feature(async_await)]

use hsr::futures3::{FutureExt, TryFutureExt};
use petstore::{
    api::{DeletePetError, GetPetError},
    client::Client,
    NewPet, PetstoreApi,
};

// Run the client via a CLI
fn main() {
    env_logger::init();
    let client = Client::new("http://localhost:8000".parse().unwrap());
    let fut = run(&client);
    hsr::actix_rt::System::new("main")
        .block_on(fut.boxed_local().compat())
        .unwrap();
}

fn dbg(v: impl std::fmt::Debug) -> String {
    format!("{:?}", v)
}

async fn run(client: &Client) -> Result<(), String> {
    // Create two pets
    let pet1 = NewPet {
        name: "Alex the Goat".into(),
        tag: None,
    };
    let pet2 = NewPet {
        name: "Bob the Badger".into(),
        tag: None,
    };

    let () = client.create_pet(pet1).map_err(dbg).await?;
    let () = client.create_pet(pet2).map_err(dbg).await?;

    // Fetch a pet
    let pet = client.get_pet(0).map_err(dbg).await?;
    println!("Got pet: {:?}", pet);

    // Fetch all pets
    let pets = client.get_all_pets(10, None).map_err(dbg).await?;
    println!("Got pets: {:?}", pets);

    // Fetch a pet that doesn't exist
    // Note the custom return error
    if let Err(GetPetError::NotFound) = client.get_pet(500).await {
        ()
    } else {
        panic!("Not not found")
    };

    // Empty the DB
    let () = client.delete_pet(0).map_err(dbg).await?;
    let () = client.delete_pet(0).map_err(dbg).await?;
    if let Err(DeletePetError::NotFound) = client.delete_pet(0).await {
        ()
    } else {
        panic!("Not not found")
    };

    Ok(())
}
