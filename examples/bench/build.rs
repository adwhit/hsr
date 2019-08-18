use hsr_codegen;
use std::io::Write;

fn main() {
    let code = hsr_codegen::generate_from_yaml_file("bench.yaml").expect("Generation failure");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir).join("api.rs");
    let mut f = std::fs::File::create(&dest_path).unwrap();

    write!(f, "{}", code).unwrap();
}
