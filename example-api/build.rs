use hsr_codegen;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

fn main() {
    let code = hsr_codegen::generate_from_yaml_file("petstore.yaml").expect("Generation failure");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("api.rs");
    let mut f = File::create(&dest_path).unwrap();

    write!(f, "{}", code).unwrap();
}
