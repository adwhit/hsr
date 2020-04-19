use std::path::PathBuf;

use structopt::StructOpt;

use hsr_codegen::generate_from_yaml_file;

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(parse(from_os_str))]
    spec: PathBuf,
}

fn main() {
    let args = Args::from_args();
    println!("{:?}", args);

    let gen = generate_from_yaml_file(args.spec).unwrap();

    println!("{}", gen);
}
