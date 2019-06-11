use serverfull;

fn main() {
    serverfull::generate_from_yaml_path("petstore-expanded.yaml").expect("Generation failure");
}
