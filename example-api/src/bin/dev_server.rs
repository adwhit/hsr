use example_api::{my_api::serve, Api};

fn main() -> Result<(), std::io::Error> {
    serve::<Api>("localhost:8000")
}
