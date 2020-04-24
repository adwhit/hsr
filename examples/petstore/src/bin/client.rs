use std::error::Error;

use hsr::futures::TryFutureExt;
use petstore::{api, client::Client, NewPet};

// This CLI uses the auto-generated API client to test various server endpoints

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let client = Client::new("http://localhost:8000".parse().unwrap());
    run(&client).await
}

fn dbg(v: impl std::fmt::Debug) -> String {
    format!("{:?}", v)
}

async fn run(client: &Client) -> Result<(), Box<dyn Error>> {
    // Create two pets
    let pet1 = NewPet {
        name: "Alex the Goat".into(),
        tag: None,
    };
    let pet2 = NewPet {
        name: "Bob the Badger".into(),
        tag: None,
    };

    let _ = client.create_pet(pet1).map_err(dbg).await?;
    let _ = client.create_pet(pet2).map_err(dbg).await?;

    // Fetch a pet
    let pet = client.get_pet(0).await.map_err(dbg)?;
    println!("Got pet: {:?}", pet);

    // Fetch all pets
    let pets = client.get_all_pets(10, None).map_err(dbg).await?;
    println!("Got pets: {:?}", pets);

    // Fetch a pet that doesn't exist
    // Note the custom return error
    if let api::GetPet::NotFound = client.get_pet(500).await? {
        ()
    } else {
        panic!("Not not found")
    };

    // Empty the DB
    let _ = client.delete_pet(0).map_err(dbg).await?;
    let _ = client.delete_pet(0).map_err(dbg).await?;
    if let api::DeletePet::NotFound = client.delete_pet(0).await? {
        ()
    } else {
        panic!("Not not found")
    };

    Ok(())
}
