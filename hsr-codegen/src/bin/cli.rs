use std::path::PathBuf;

use structopt::StructOpt;

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(parse(from_os_str))]
    spec: PathBuf,
}

fn main() {
    let args = Args::from_args();
    println!("{:?}", args);
}
