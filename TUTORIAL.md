# Tuturial

`hsr` is a code-generator. It takes an OpenAPIv3 spec as an input and generates Rust
as it's output. It can massively reduce the boilerplate and general annoyance of writing
an HTTP API, either for an internet-facing web service or an internal micro (or macro!) service.
And becuase it is just a thin layer on top of `actix-web`, performance is excellent.

OpenAPIv3, aka swagger, is a popular format for specifying an HTTP API.
If you aren't familiar with OpenAPI, I recommend having at least a
[quick skim of the the docs](https://swagger.io/docs/specification/about/) before proceeding.

The generated Rust code presents an interface (i.e. a trait) which the user should implement.
Once that trait is implemented, the server is ready to go. `hsr` takes care of
bootstrapping the server, url routing, serializing the inputs and outputs and other
boilerplate.

Rust's powerful type system makes this a particularly nice workflow because 'if it compiles,
it works'. Suppose you need to modify or add an endpoint, you simply modify you API spec 
(usually a `.yaml` file) and recompile. `rustc` will most likely throw a bunch of type
errors, you fix them, and you're done.

Right, enough talk, lets get started. Make a new project.

``` sh
cargo new tutorial
cd tutorial
```
We'll use the handy [`cargo-edit`](https://crates.io/crates/cargo-edit) to add our
dependencies.

``` sh
cargo add -B hsr-codegen   # build dependency
cargo add hsr serde        # serde is MANDATORY
```

First, we'll define an api. Create a `spec.yaml` file containing the following:
``` yaml
# spec.yaml
openapi: "3.0.0"
info:
  version: 0.0.1
  title: hsr-tutorial
servers:
  - url: http://localhost:8000

paths:
  /hello:
    get:
      operationId: hello
      responses:
        '200':
          description: Yes, we get it, hello
```

This is just about the simplest possible API. It exposes a single route, `GET /hello`,
to which it responds with a `200 OK`. That's it. Yes, I know it's boring, we'll make it
WACKY later - for now we're just going to build it.

Create a `build.rs` file in your project root:

```rust
// build.rs

use hsr_codegen;
use std::io::Write;

fn main() {
    let code = hsr_codegen::generate_from_yaml_file("spec.yaml").expect("Generation failure");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir).join("api.rs");
    let mut f = std::fs::File::create(&dest_path).unwrap();

    write!(f, "{}", code).unwrap();
    println!("cargo:rerun-if-changed=spec.yaml");
}
```
Now if we run `cargo build`, it does... something. Specifically, we just told Rust
that at build-time:
* Open the spec file
* Generate our interface code from the spec
* Find the magic OUT_DIR and write the code to `$OUT_DIR/api.rs`

Nice! But not very useful as-is. To actually use this code, we need to get it into our
project source.

Create a file `src/lib.rs` containing the following:

```rust
pub mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}
```
Now we can compile! Go! Or, actually, wait a sec. We are going to codegen an interface.
Really, we want to be able to view our API. But viewing the raw code isn't very
enlightening (and Rust doesn't make it easy), instead we view it in `rustdoc`.
``` rust
$ cargo doc --open
```
Ok, this time it did something useful.
