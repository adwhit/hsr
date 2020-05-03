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
cargo add -B hsr-codegen        # build dependency
cargo add hsr actix-rt serde    # runtime dependencies
```

## Hello HSR!

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
    // If we alter the spec.yaml, we should rebuild the api
    println!("cargo:rerun-if-changed=spec.yaml");
}
```
Now if we run `cargo build`, it does... something. Specifically, we just told Rust
that at build-time it should:
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
Ok, this time it did something useful. Inside the `api` module we can see a promising-sounding
things like `client` and `server` modules and an `HsrTutorialApi` trait.

The `HsrTutorialApi` trait has a rather intimidating definition, something like:

``` rust
trait HsrTutorialApi {
    fn hello<'life0, 'async_trait>(&'life0 self) -> Pin<Box<dyn Future<Output = Hello> + 'async_trait>>
    where
      'life0: 'async_trait,
      Self: 'async_trait;
}
```
This is not as complicated as it looks. Basically this trait should be read as:

```rust
trait HsrTutorialapi {
    async fn hello(&self) -> Hello;
}
```
where `Hello` is defined elsewhere. Which we can see closely matches the definition in `spec.yaml`.

Why does the definition... not look like that? Well, unfortunately "async-in-traits" is not yet
supported [(issue)](https://github.com/rust-lang/rfcs/issues/2739) so for now we work around it
with the amazing [`async-trait`](https://github.com/dtolnay/async-trait),
which however gives us these slightly inscrutable api definitions.

Lets gloss over this for now and implement the trait. Continuing in `src/lib.rs`:

```rust
// src/lib.rs

/* .. previous code .. */

struct Api;

#[hsr::async_trait::async_trait(?Send)]
impl api::HsrTutorialApi for Api {
    async fn hello(&self) -> api::Hello {
        api::Hello::Ok
    }
}
```
Notice that we've used the `async_trait` macro (conveniently re-exported from hsr)
to allow us to implement the trait as we would 'like' it to be written, rather than as it is defined
according to the docs.

The last step is to serve our api. We can see from the api docs that there is a function
in `tutorial::api::server` with the signature 

``` rust
pub async fn serve<A: HsrTutorialApi>(api: A, cfg: Config) -> std::io::Result<()>
```
so let's use that.


In `src/main.rs`:

``` rust
use tutorial::{api, Api};

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    let config = hsr::Config::with_host("http://localhost:8000".parse().unwrap());
    api::server::serve(Api, config).await
}
```

That's it, the webserver is ready. We're going to test it with [`httpie`](https://httpie.org/), which
can be installed with `pip3 install httpie --user`.

``` sh
// terminal 1
cargo run

// terminal 2
➜  ~ http --print hH :8000/hello
GET /hello HTTP/1.1
# ...

HTTP/1.1 200 OK
```
It lives!

## That's not fun, make it fun

Fine, very nice. What else can this thing do? Let's flex our muscles.

Add the following to you `spec.yaml`:


```yaml
# spec.yaml

  # ... previous code ...

  /greet/{name}:
    get:
      operationId: greet
      parameters:
        - name: name
          in: path
          required: true
          schema:
            type: string
        - name: obsequiousness
          in: query
          required: false
          schema:
            type: integer
      responses:
        '200':
          description: If you can't say something nice...
          content:
            application/json:
              schema:
                type: object
                required:
                  - greeting
                properties:
                  greeting:
                    type: string
                  lay_it_on_thick:
                    $ref: '#/components/schemas/LayItOnThick'

components:
  schemas:
    LayItOnThick:
      type: object
      required:
        - is_wonderful_person
        - is_kind_to_animals
        - would_take_to_meet_family
      properties:
        is_wonderful_person:
          type: boolean
        is_kind_to_animals:
          type: boolean
        would_take_to_meet_family:
          type: boolean
```

Now if we re-run `cargo doc` and refresh our browser, we have some new goodies. In the trait definition:

``` rust
trait HsrTutorialApi {
    // .. previous definition

    fn greet<'life0, 'async_trait>(&'life0 self, name: String, obsequiousness_level: Option<i64>)
        -> Pin<Box<dyn Future<Output = Greet> + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
```
... which we have learned should be read as

``` rust
trait HsrTutorialapi {
    // ...

    async fn greet(&self, name: String, obsequiosness_level: Option<i64>) -> Greet;
}
```

We implement it like so:


``` rust
#[hsr::async_trait::async_trait(?Send)]
impl api::HsrTutorialApi for Api {
    // ... previous

    async fn greet(&self, name: String, obsequiousness_level: Option<i64>) -> api::Greet {
        let obs_lvl = obsequiousness_level.unwrap_or(0);
        let lay_it_on_thick = if obs_lvl <= 0 {
            None
        } else {
            Some(api::LayItOnThick {
                is_wonderful_person: obs_lvl >= 1,
                is_kind_to_animals: obs_lvl >= 2,
                would_take_to_meet_family: obs_lvl >= 3,
            })
        };
        api::Greet::Ok(api::Greet200 {
            greeting: format!("Greetings {}, pleased to meet you", name),
            lay_it_on_thick,
        })
    }
}
```

That's our new endpoint implemented. Let's try it out:

``` sh
➜  ~ http ":8000/greet/Alex"
HTTP/1.1 200 OK

{
    "greeting": "Greetings Alex, pleased to meet you",
    "lay_it_on_thick": null
}

➜  ~ http ":8000/greet/Alex?obsequiousness=1"
HTTP/1.1 200 OK

{
    "greeting": "Greetings Alex, pleased to meet you",
    "lay_it_on_thick": {
        "is_kind_to_animals": false,
        "is_wonderful_person": true,
        "would_take_to_meet_family": false
    }
}

➜  ~ http ":8000/greet/Alex?obsequiousness=50"
HTTP/1.1 200 OK

{
    "greeting": "Greetings Alex, pleased to meet you",
    "lay_it_on_thick": {
        "is_kind_to_animals": true,
        "is_wonderful_person": true,
        "would_take_to_meet_family": true
    }
}
``` sh

Beautiful! This API really knows how make you feel good about yourself.

That's it for now. Take a look at the `petstore` example for a more complex spec
that implements a somewhat-realistic looking API.
