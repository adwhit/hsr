# Serverless Done Good


* I am the 90% use case
* I don't want lots of options for API design. I want one simple, well supported way which WORKS
* And by WORKS I mean is SIMPLE and TYPESAFE and therefore HARD TO SCREW UP
* I define my interface with OpenAPI3 (possibly a constrained version of it, since it has too many knobs)
* I use a tool to codegen a trait.
* It codegens a client too.
* I implement this trait. In doing so I have fulfilled the API interface.
* I upload the code to the platform. The platform will validate and compile the code (or else it will barf).
* The code is now deployed. The endpoints are live at a given IP
* I am charged based on how many seconds of compute I use (I can set timeouts)
* If I write async code, I am not charged for non-blocking IO (i.e. database lookups)
* If I want to use a websocket, I will


## Stuff that I don't care about that is handled for me

* HTTPS
* Auto-scaling
* Authentication
* Errors (if my code crashes, the endpoint returns `500` and keeps going)
* Logging
* Metrics/Alerts
* Swagger UI is auto-hosted

## State

* Obviously most APIs have some kind of state, if only a list of users and (hashed) passwords.
* State should be held in a database.
* So, can we have 'database done right'?
* Definitely a tricker problem. Aurora is probably the model
* 'Postgres, but without the bother'

# Design

* How exactly should codegen be done? Ideally, the only artifacts are the swagger.yaml file
  and the implementation.
  
```yaml
openapi: "3.0.0"
info:
  title: Some Api
  version: 0.1
paths:
  /foo:
    get:
      summary: Get foo
...
```
```rust
use some_api;

struct MyApi;

impl some_api::Api for MyApi {
    fn get_foo(id: i32) -> Box<Future<Item = some_api::Foo>> { /*..*/ }
    // ...
}
```

Difficulty: we obviously need to codegen the api interface locally so that the
user can fulfill the definition. But equally, we don't want to 'hand off' this part
of the build completely to the user, otherwise the host won't be able to check that
the code is valid.

But using a `build.rs` to do the codegen is very convenient for the user, so 
we'd like to keep it.

In other words, the host wants to work like this:
* Receive uploaded `openapi.yaml`
* Receive uploaded `my-api.tar.gz`
* Generate scaffolding code (the actual server code!)
* Place the implementation inside the codegenned directory
* Compile the whole package

Result is a standalone binary that can be run in a sandbox (and has hooks for monitoring, etc).

How can we achieve this? Easy, we have two different versions of codegen, one used by the
host, one available on `crates.io`.
When compiled by the client, they get everything they need for functional code
including a simple development server. When compiling on the host, we have
all the extra stuff necessary for serverless magic (HTTPS, auth, billing etc).

### Design details

The OpenAPI spec allows one to return multiple different types from each operation.
How should this be represented? The most 'obvious' way it to just return an enum
which may contain any of the outcomes.

But, we would really like to handle the sad path in an idiomatic way with the `?` operator.
Suppose the responses were

```
200: Happy
401: NotAllowed
406: NotAcceptable
```
We would like the method:
```
fn api_call(&self, something: SomeData) -> Result<Happy, ApiCallError>;
```
with the impls:
```
enum ApiCallError {
    NotAllowed(NotAllowed),
    NotAcceptable(NotAcceptable)
}

impl From<NotAllowed> for ApiCallError {
    // ...
}

impl From<NotAcceptable> for ApiCallError {
    // ...
}

impl HasStatus for Result<Happy, ApiCallError> {
    fn status_code(&self) -> StatusCode {
        match self {
            Ok(_) => 200,
            Err(NotAllowed) => 401,
            Err(NotAcceptable) => 406,
        }
    }
}
```

Then we use it like
```
fn do_something(s: SomeData) -> Result<Other, NotAllowed> { .. }

fn do_another_thing(o: Other) -> Result<Happy, NotAcceptable> { .. }

fn api_call(&self, something: SomeData) -> Result<Happy, ApiCallError> {
    let other = do_something(something)?;
    let happy = do_another_thing(other)?;
    Ok(happy)
}
```
